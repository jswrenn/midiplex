use std::io;
use std::net::{UdpSocket, SocketAddr};
use types::*;
use outputs::Output;

/// An output backed by a UDP socket.
pub struct UdpOutput {
  pub addr  : SocketAddr,
  pub socket: UdpSocket
}

impl Output for UdpOutput
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

impl Drop for UdpOutput {
  fn drop(&mut self) {
    let _ = self.silence();
  }
}
