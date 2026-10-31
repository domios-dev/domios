use crate::backend::{
    ActiveWatch, BatchHandler, RescanSink, RescanWatcher, WatchSignal, WatchSpec, REPLACEMENT_SINK,
    RESCAN_SINK,
};
use crate::macos_stream::{NativeEvent, NativePort, Stream};
use crate::stream_plan::{self, StreamSpec};
use notify_debouncer_mini::{new_debouncer_opt, Config, DebounceEventResult};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{mpsc, Arc};

enum Command {
    Set(Vec<WatchSpec>, mpsc::Sender<Result<(), String>>),
    Event(NativeEvent),
    Signal(WatchSignal),
    Stop,
}

struct WatchSet {
    commands: mpsc::Sender<Command>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl ActiveWatch for WatchSet {
    fn update(&mut self, specs: &[WatchSpec]) -> Result<(), String> {
        let (reply, result) = mpsc::channel();
        self.commands
            .send(Command::Set(specs.to_vec(), reply))
            .map_err(|error| error.to_string())?;
        result.recv().map_err(|error| error.to_string())?
    }
}

impl Drop for WatchSet {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn desired(specs: &[WatchSpec]) -> Result<BTreeSet<StreamSpec>, String> {
    let mut streams = BTreeSet::new();
    for spec in specs.iter().filter(|spec| spec.partitioned) {
        streams.extend(stream_plan::plan(
            &spec.path,
            &crate::ignore_plan::plan(&spec.path)?,
        )?);
    }
    Ok(streams)
}

fn reconcile(
    port: &mut NativePort,
    streams: &mut BTreeMap<StreamSpec, Stream>,
    specs: &[WatchSpec],
) -> Result<bool, String> {
    let result = (|| {
        let planned = desired(specs)?;
        let changed = streams.keys().cloned().collect::<BTreeSet<_>>() != planned;
        for spec in &planned {
            if !streams.contains_key(spec) {
                streams.insert(spec.clone(), stream_plan::start(port, spec)?);
            }
        }
        streams.retain(|spec, _| planned.contains(spec));
        Ok(changed)
    })();
    if result.is_err() {
        streams.clear();
    }
    result
}

fn metadata(specs: &[WatchSpec]) -> Vec<WatchSpec> {
    specs
        .iter()
        .filter(|spec| !spec.partitioned)
        .cloned()
        .collect()
}

pub(crate) fn build(
    specs: &[WatchSpec],
    handler: BatchHandler,
) -> Result<Box<dyn ActiveWatch>, String> {
    let (dispatch, signals) = mpsc::channel();
    std::thread::Builder::new()
        .name("repository-watch-dispatch".into())
        .spawn(move || {
            let mut handler = handler;
            while let Ok(signal) = signals.recv() {
                handler(signal);
            }
        })
        .map_err(|error| error.to_string())?;
    let (commands, incoming) = mpsc::channel();
    let events = commands.clone();
    let initial = specs.to_vec();
    let (ready, started) = mpsc::channel();
    let thread = std::thread::Builder::new()
        .name("repository-fsevents-set".into())
        .spawn(move || {
            let rescan = events.clone();
            let sink: RescanSink = Arc::new(move || {
                let _ = rescan.send(Command::Signal(WatchSignal::Rescan));
            });
            RESCAN_SINK.with(|slot| *slot.borrow_mut() = Some(sink));
            let replacement = events.clone();
            REPLACEMENT_SINK.with(|slot| {
                *slot.borrow_mut() = Some(Arc::new(move |paths| {
                    let _ = replacement.send(Command::Event(NativeEvent {
                        paths,
                        rescan: false,
                        topology: true,
                    }));
                }))
            });
            let batched = events.clone();
            let built = new_debouncer_opt::<_, RescanWatcher>(
                Config::default().with_timeout(crate::DEBOUNCE),
                move |result: DebounceEventResult| match result {
                    Ok(events) => {
                        let paths = events.into_iter().map(|event| event.path).collect();
                        let _ = batched.send(Command::Event(NativeEvent {
                            paths,
                            rescan: false,
                            topology: false,
                        }));
                    }
                    Err(error) => {
                        let _ =
                            batched.send(Command::Signal(WatchSignal::Failed(error.to_string())));
                    }
                },
            );
            RESCAN_SINK.with(|slot| *slot.borrow_mut() = None);
            REPLACEMENT_SINK.with(|slot| *slot.borrow_mut() = None);
            let mut metadata_watcher = match built {
                Ok(watcher) => watcher,
                Err(error) => {
                    let _ = ready.send(Err(error.to_string()));
                    return;
                }
            };
            let native = events.clone();
            let mut port = NativePort::new(move |event| {
                let _ = native.send(Command::Event(event));
            });
            let mut streams = BTreeMap::new();
            let mut active_metadata = BTreeMap::new();
            let mut specs = initial;
            let result = reconcile(&mut port, &mut streams, &specs).and_then(|_| {
                crate::subscriptions::reconcile(
                    metadata_watcher.watcher(),
                    &mut active_metadata,
                    &metadata(&specs),
                )
                .map(|_| ())
            });
            let failed = result.is_err();
            if ready.send(result).is_err() || failed {
                return;
            }
            while let Ok(command) = incoming.recv() {
                match command {
                    Command::Stop => break,
                    Command::Signal(signal) => {
                        let _ = dispatch.send(signal);
                    }
                    Command::Set(updated, reply) => {
                        let result =
                            reconcile(&mut port, &mut streams, &updated).and_then(|changed| {
                                crate::subscriptions::reconcile(
                                    metadata_watcher.watcher(),
                                    &mut active_metadata,
                                    &metadata(&updated),
                                )
                                .map(|metadata| changed || metadata)
                            });
                        if matches!(result, Ok(true)) {
                            let _ = dispatch.send(WatchSignal::CoverageChanged);
                        }
                        if result.is_ok() {
                            specs = updated;
                        }
                        if let Err(error) = &result {
                            let _ = dispatch.send(WatchSignal::Failed(error.clone()));
                        }
                        let _ = reply.send(result.map(|_| ()));
                    }
                    Command::Event(event) => {
                        if event.rescan {
                            let _ = dispatch.send(WatchSignal::Rescan);
                        }
                        if event.topology
                            || crate::subscriptions::topology_changed(
                                &specs,
                                &active_metadata,
                                &event.paths,
                            )
                        {
                            let before = streams.len();
                            if event.topology {
                                streams.retain(|spec, _| {
                                    !event.paths.iter().any(|path| spec.root.starts_with(path))
                                });
                            }
                            let rearmed = streams.len() != before;
                            match reconcile(&mut port, &mut streams, &specs) {
                                Ok(changed) => {
                                    let metadata_signals = crate::backend::refresh_coverage(
                                        metadata_watcher.watcher(),
                                        &mut active_metadata,
                                        &metadata(&specs),
                                        &event.paths,
                                        event.topology,
                                    );
                                    if changed || rearmed {
                                        let _ = dispatch.send(WatchSignal::CoverageChanged);
                                    }
                                    for signal in metadata_signals {
                                        let _ = dispatch.send(signal);
                                    }
                                }
                                Err(error) => {
                                    log::warn!(
                                        "[repowatch] FSEvents subscription refused: {error}"
                                    );
                                    let _ = dispatch.send(WatchSignal::Failed(error));
                                }
                            }
                        }
                        if !event.paths.is_empty() {
                            let _ = dispatch.send(WatchSignal::Batch(event.paths));
                        }
                    }
                }
            }
        })
        .map_err(|error| error.to_string())?;
    let watch = WatchSet {
        commands,
        thread: Some(thread),
    };
    started.recv().map_err(|error| error.to_string())??;
    Ok(Box::new(watch))
}
