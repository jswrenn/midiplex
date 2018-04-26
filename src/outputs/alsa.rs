use std::ffi::CString;
use alsa;
use alsa::seq::{self, Seq, Event};
use alsa::seq::{EvNote, EventType};
use types::*;
use outputs::Output;

/// An output backed by an ALSA output port.
pub struct AlsaOutput<'s> {
  pub port: Port,
  pub sequencer: &'s Seq
}

impl<'s> AlsaOutput<'s> {
  pub fn new<N>(sequencer : &'s Seq, name: N)
      -> Result<AlsaOutput<'s>, alsa::Error>
    where N: Into<Vec<u8>>
  {
    let port =
      sequencer.create_simple_port(
          &CString::new(name).unwrap(),
          seq::READ | seq::SUBS_READ,
          seq::MIDI_GENERIC | seq::APPLICATION)?;
    Ok(AlsaOutput { port, sequencer })
  }
}

impl<'s> Output for AlsaOutput<'s>
{
  type Error = alsa::Error;

  fn on(&mut self, note: Note, channel: Channel, velocity: Velocity)
      -> Result<(), Self::Error>
  {
    let mut event = Event::new(EventType::Noteon,
      &EvNote {note, channel, velocity, ..Default::default() });
    event.set_source(self.port);
    event.set_subs();
    event.set_direct();
    self.sequencer.event_output_direct(&mut event)?;
    Ok(())
  }

  fn off(&mut self, note: Note, channel:Channel)
      -> Result<(), Self::Error>
  {
    let mut event = Event::new(EventType::Noteoff,
      &EvNote {note, channel, ..Default::default() });
    event.set_source(self.port);
    event.set_subs();
    event.set_direct();
    self.sequencer.event_output_direct(&mut event)?;
    Ok(())
  }
}