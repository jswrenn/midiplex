use types::*;
use outputs::Output;

impl<'a> Output for &'a str
{
  type Error = ();

  fn on(&mut self, _note: Note, _channel: Channel, _velocity: Velocity)
      -> Result<(), Self::Error>
  {
    Ok(())
  }

  fn off(&mut self, _note: Note, _channel:Channel)
      -> Result<(), Self::Error>
  {
    Ok(())
  }
}
