pub type Note     = u8;
pub type Channel  = u8;
pub type Velocity = u8;

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