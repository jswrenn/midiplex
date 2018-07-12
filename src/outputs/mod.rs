use types::*;

pub mod udp;
pub mod alsa;
pub mod midiplex;

#[cfg(test)] pub mod dummy;

pub use self::udp::UdpOutput;
pub use self::alsa::AlsaOutput;
pub use self::midiplex::Midiplexer;

pub trait Output
{
  type Error;

  fn on(&mut self, note: Note, channel: Channel, velocity: Velocity)
      -> Result<(), Self::Error>;

  fn off(&mut self, note: Note, channel:Channel)
      -> Result<(), Self::Error>;

  fn silence(&mut self)
      -> Result<(), Self::Error>
  {
    for channel in 0..16 {
      for note in 0..128 {
        self.off(note, channel)?;
      }
    }
    Ok(())
  }
}