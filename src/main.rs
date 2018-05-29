#![feature(never_type)]
#![feature(attr_literals)]
#![feature(alloc_system)]
extern crate alloc_system;

#[macro_use] extern crate structopt;
extern crate clap;
extern crate nix;
extern crate alsa;
extern crate indexmap;

use alsa::seq;
use std::ffi::CString;
use clap::AppSettings;
use structopt::StructOpt;
use std::net::{UdpSocket, ToSocketAddrs};

mod types;
mod outputs;
use outputs::Output;


/// Output Mode
#[derive(StructOpt)]
#[structopt(name = "MODE")]
enum Mode {
  /// UDP output mode
  #[structopt(name = "udp")]
  UDP {
    /// space-delimited socket addresses of hosts to send events to
    #[structopt(name = "HOSTS")]
    hosts: Vec<String>,
  },
  /// ALSA output mode
  #[structopt(name = "alsa")]
  ALSA {
    /// output pool size
    #[structopt(short = "o", long = "output-pool-size", name="O")]
    output_pool_size: Option<u32>,

    /// space-delimited names of output ports
    #[structopt(name = "NAMES", raw(required = "true"))]
    names: Vec<String>,
  }
}


#[derive(StructOpt)]
#[structopt(raw(setting = "AppSettings::DisableHelpSubcommand"))]
struct Options {
  /// set the ALSA input pool size
  #[structopt(short = "i", long = "input-pool-size", name="I")]
  input_pool_size: Option<u32>,

  /// set the maximum number of outputs allocated to any note
  #[structopt(short = "m", long = "max-allocation", name="N")]
  max_allocation: Option<usize>,

  /// set the output mode
  #[structopt(subcommand,name="MODE")]
  output: Mode
}


/// wait for a note on `input` and forward it to `output`
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


/// repeatedly wait for a note on `input` and forward it to `output`
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


/// distribute notes from `input` to `outputs`
fn midiplex<'i, 's, O>(input: &'i mut alsa::seq::Input<'s>, outputs: O, max_allocation: Option<usize>)
    -> Result<!, alsa::Error>
  where O: IntoIterator,
        O::Item: Output
{
  let mut output = outputs::Midiplexer::new(outputs, max_allocation);
  forward_all(input, &mut output)
}


fn run(options : Options) -> Result<!, alsa::Error> {
  // initialize the ALSA sequencer client
  let sequencer_name = CString::new(env!("CARGO_PKG_NAME")).unwrap();
  let sequencer = alsa::Seq::open(None, None, true)?;
  sequencer.set_client_name(&sequencer_name)?;

  // initialize midi input port
  sequencer.create_simple_port(
    &CString::new("input").unwrap(),
    seq::WRITE | seq::SUBS_WRITE,
    seq::MIDI_GENERIC | seq::APPLICATION)?;

  if let Some(size) = options.input_pool_size {
    sequencer.set_client_pool_input(size)?
  }

  // capture the input stream of events
  let mut input = sequencer.input();

  match options.output {
    Mode::UDP{hosts} =>
      {
        midiplex(&mut input,
          hosts.iter().map(|host|
            outputs::UdpOutput {
              addr: host.to_socket_addrs().unwrap().next().unwrap(),
              socket: UdpSocket::bind("0.0.0.0:0").unwrap(),
            }), options.max_allocation)
      },
    Mode::ALSA{names, output_pool_size} =>
      {
        if let Some(size) = output_pool_size {
          sequencer.set_client_pool_output(size)?;
        }
        midiplex(&mut input,
          names.into_iter().map(|name|
            outputs::AlsaOutput::new(&sequencer, name).unwrap()),
          options.max_allocation)

      },
  }
}


fn main() {
  let options = Options::from_args();
  // run and, if necessary, print error message to stderr
  if let Err(error) = run(options) {
    eprintln!("Error: {}", error);
    std::process::exit(1);
  }
}
