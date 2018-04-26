use types::*;
use std::io;
use std::net::{UdpSocket, SocketAddr};

/// An output backed by a UDP socket.
pub struct UdpOutput<'s> {
  pub addr  : SocketAddr,
  pub socket: &'s UdpSocket
}

impl<'s> Output for UdpOutput<'s>
{
  type Error = io::Error;

  fn on(&mut self, note: Note, channel: Channel, velocity: Velocity)
      -> Result<(), Self::Error>
  {
    self.socket.send_to(&[0x90 | channel, note, velocity], self.addr)
      .map(|_| ())
  }

  fn off(&mut self, note: Note, channel:Channel)
      -> Result<(), Self::Error>
  {
    self.socket.send_to(&[0x80 | channel, note, 0], self.addr)
      .map(|_| ())
  }
}

impl<'s> Drop for UdpOutput<'s> {
  fn drop(&mut self) {
    let _ = self.silence();
  }
}
