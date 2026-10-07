use objc2_foundation::NSArray;
use objc2_virtualization::{
  VZAudioDeviceConfiguration, VZHostAudioOutputStreamSink, VZVirtioSoundDeviceConfiguration,
  VZVirtioSoundDeviceOutputStreamConfiguration, VZVirtioSoundDeviceStreamConfiguration,
  VZVirtualMachineConfiguration,
};

pub(super) fn attach(config: &VZVirtualMachineConfiguration, speakers: bool) {
  if !speakers {
    return;
  }
  unsafe {
    let output = VZVirtioSoundDeviceOutputStreamConfiguration::new();
    output.setSink(Some(&VZHostAudioOutputStreamSink::new()));
    let sound = VZVirtioSoundDeviceConfiguration::new();
    sound.setStreams(&NSArray::<VZVirtioSoundDeviceStreamConfiguration>::from_slice(&[&output]));
    config.setAudioDevices(&NSArray::<VZAudioDeviceConfiguration>::from_slice(&[
      &sound,
    ]));
  }
}
