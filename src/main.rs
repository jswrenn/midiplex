#![feature(alloc_system)]
extern crate alloc_system;

#[macro_use] extern crate structopt;
extern crate nix;
extern crate alsa;
extern crate indexmap;

use alsa::seq;
use std::ffi::CString;
use structopt::StructOpt;
use std::iter::FromIterator;
use std::net::{UdpSocket, ToSocketAddrs};

mod types;
mod midiplex;
mod udp;

use types::*;
use udp::*;
use midiplex::*;

type Port = i32;


#[derive(StructOpt)]
struct Options {
  /// set the input pool size
  #[structopt(short = "i", long = "input-pool-size")]
  input_pool_size: Option<u32>,
  /// addresses of clients to connect to
  #[structopt(name = "HOSTS")]
  hosts: Vec<String>,
}


/// create an ALSA virtual MIDI input port on this sequencer
fn input(seq: &alsa::Seq) -> Result<Port, alsa::Error> {
  let mut port_info = seq::PortInfo::empty()?;
  let port_name = CString::new("input").unwrap();
  port_info.set_name(&port_name);
  port_info.set_capability(seq::WRITE | seq::SUBS_WRITE);
  port_info.set_type(seq::MIDI_GENERIC | seq::APPLICATION);

  seq.create_port(&port_info)?;

  Ok(port_info.get_port())
}

fn forward<'s, 'i, 'o, O>(input: &'i mut alsa::seq::Input<'s>, output: &'o mut O)
  -> Result<(), alsa::Error>
  where O: Output
{
  if input.event_input_pending(true)? == 0 {
    return Ok(());
  }

  let event = input.event_input()?;

  match event.get_type() {
    seq::EventType::Noteon =>
      {
        let data: seq::EvNote = event.get_data().unwrap();
        if data.velocity > 0 {
          let _ = output.on(data.note, data.channel, data.velocity);
        } else {
          let _ = output.off(data.note, data.channel);
        }
      },
    seq::EventType::Noteoff =>
      {
        let data: seq::EvNote = event.get_data().unwrap();
        let _ = output.off(data.note, data.channel);
      }
    _ => {}
  }
  Ok(())
}

fn forward_all<'s, 'i, 'o, O>(input: &'i mut alsa::seq::Input<'s>, output: &'o mut O)
  -> Result<!, alsa::Error>
  where O: Output
{
  loop {
    let result = forward(input, output);
    match result {
      Ok(_) => continue,
      Err(error) => {
        if error.errno() == Some(nix::Errno::ENOSPC) {
          eprintln!("{:?}", error);
          let _ =output.silence();
        } else if error.errno() == Some(nix::Errno::EAGAIN) {
          continue;
        } else {
          return Err(error);
        }
      }
    }
  }
}

fn run_udp(sequencer: &alsa::Seq, hosts: Vec<String>)
  -> Result<!, alsa::Error>
{
  let socket = UdpSocket::bind("0.0.0.0:0").unwrap();

  let mut output =
    Midiplexer::from_iter(
      hosts.iter()
        .map(|host|
          UdpOutput {
            addr: host.to_socket_addrs().unwrap().next().unwrap(),
            socket: &socket
          }));

  forward_all(&mut sequencer.input(), &mut output)
}

fn run(options : Options) -> Result<!, alsa::Error> {
  // initialize the ALSA sequencer client
  let sequencer_name = CString::new(env!("CARGO_PKG_NAME")).unwrap();
  let sequencer = alsa::Seq::open(None, None, true)?;
  sequencer.set_client_name(&sequencer_name)?;
  if let Some(size) = options.input_pool_size {
    sequencer.set_client_pool_input(size)?
  }
  input(&sequencer)?;

  run_udp(&sequencer, options.hosts)
}


fn main() {
  let options = Options::from_args();
  // run and, if necessary, print error message to stderr
  if let Err(error) = run(options) {
    eprintln!("Error: {}", error);
    std::process::exit(1);
  }
}
