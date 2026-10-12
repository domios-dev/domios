import { useEffect, useState } from "react";
import { invoke } from "../../../lib/bridge";

interface RecordingInput {
  inputId: string;
  collectionId: string;
}

export function LocalRecordingSetup({ pluginId, onClose }: { pluginId: string; onClose: () => void }) {
  const [inputs, setInputs] = useState<RecordingInput[]>([]);
  const [microphones, setMicrophones] = useState<MediaDeviceInfo[]>([]);
  const [inputId, setInputId] = useState("");
  const [microphoneId, setMicrophoneId] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => {
    let active = true;
    void invoke<RecordingInput[]>("local_recording_input_devices", { pluginId }).then(async (selected) => {
      if (!active || selected.length === 0) return;
      setInputs(selected);
      const devices = await navigator.mediaDevices.enumerateDevices();
      if (active) setMicrophones(devices.filter((device) => device.kind === "audioinput" && device.deviceId));
    }).catch((cause) => { if (active) setError(String(cause)); });
    return () => { active = false; };
  }, [pluginId]);
  if (inputs.length === 0) return null;
  async function save() {
    const selected = inputs.find((input) => input.inputId === inputId);
    if (!selected || !microphones.some((microphone) => microphone.deviceId === microphoneId)) return;
    setSaving(true);
    try {
      await invoke("local_recording_select_devices", { pluginId, ...selected, microphoneId });
      onClose();
    } catch (cause) {
      setError(String(cause));
    } finally {
      setSaving(false);
    }
  }
  return (
    <section role="dialog" aria-modal="false" aria-labelledby="local-recording-setup-title"
      className="fixed bottom-4 right-4 z-50 flex max-w-sm flex-col gap-3 rounded-lg border border-[var(--ct-border)] bg-[var(--ct-bg)] p-4 shadow-lg">
      <h2 id="local-recording-setup-title">Connect recording button</h2>
      <label>Receiver
        <select value={inputId} onChange={(event) => setInputId(event.target.value)}>
          <option value="">Select receiver</option>
          {inputs.map((input, index) => <option key={input.inputId} value={input.inputId}>LARK receiver {index + 1}</option>)}
        </select>
      </label>
      <label>Microphone
        <select value={microphoneId} onChange={(event) => setMicrophoneId(event.target.value)}>
          <option value="">Select LARK microphone</option>
          {microphones.map((microphone, index) => <option key={microphone.deviceId} value={microphone.deviceId}>
            {microphone.label || `Microphone ${index + 1}`}
          </option>)}
        </select>
      </label>
      {error && <p role="alert">{error}</p>}
      <button type="button" disabled={saving || !inputId || !microphoneId} onClick={() => void save()}>Connect</button>
      <button type="button" disabled={saving} onClick={onClose}>Cancel</button>
    </section>
  );
}
