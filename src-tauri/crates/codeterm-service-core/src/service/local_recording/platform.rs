pub(crate) use codeterm_surface::local_recording::platform::{
    input_devices, production_adapter, supported, DeviceArrivalEvent, InputUnavailable,
    LocalInputAdapter, LocalInputArrivalWatch, RecordingInputDevice, SelectedInputEvent,
};
#[cfg(test)]
pub(crate) use codeterm_surface::local_recording::platform::{
    LocalInputSubscription, SubscriptionTeardown,
};
