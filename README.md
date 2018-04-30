MIDIplex is a utility for volume-aware distributing a _polyphonic_ MIDI stream into multiple output streams. The output streams are guaranteed to be monophonic. Output streams are allocated to a note in proportion to that note's velocity, relative to the velocity of other notes being played.

## Why MIDIplex?
Music is often _polyphonic_, i.e., at times, there are multiple notes playing simultaneously. However, some synthesizers are _monophonic_, i.e., they can only voice one sound at a time. MIDIplex consumes a MIDI stream and automagically distributes it across a set number of output channels in such a way that it is guaranteed that no output will be expected to play two notes or more notes simultaneously. You can use MIDIplex to render a polyphonic MIDI across a given number of monophonic synthesizers, be it a [choir of musical floppy disks](https://www.youtube.com/watch?v=C3dU5u4xXaY), or (as I am) across a [lab of computers with `beep` installed](https://www.youtube.com/watch?v=ehpZ2GfWKe8).

For a visual depiction of how MIDIplex distributes notes, watch [this demonstration video](https://www.youtube.com/watch?v=leyjsN-FpUo), in which events from an input keyboard (top) are distributed across three output channels.

## System Requirements
MIDIplex integrates with the _Advanced Linux Sound Architecture_. MIDIplex therefore is only supported on systems running Linux.

## Usage
### Overview

```
USAGE:
    midiplex [OPTIONS] <OUTPUT MODE>

OPTIONS:
    -i, --input-pool-size <I>   sets the ALSA input pool size

OUTPUT MODES:
    alsa                        ALSA output mode
    udp                         UDP output mode
```

### Input
MIDIplex consumes input as a virtual ALSA MIDI device named ‘midiplex’ with a writable port named ‘input’. You can verify that MIDIplex is running with `aconnect`:
```
$ aconnect -l
client 0: 'System' [type=kernel]
    0 'Timer           '
    1 'Announce        '
client 14: 'Midi Through' [type=kernel]
    0 'Midi Through Port-0'
client 128: 'midiplex' [type=user,pid=6496]
    0 'input 
```

### Output
MIDIplex supports two output modes. For most users, the ALSA output mode offers the most flexibility.

#### ALSA Output
In the ALSA output mode, midiplex creates a specified list of readable ports to which it distributes notes. You can then patch those notes to other MIDI devices using the `aconnect` utility from `alsa-utils`, or a visual connection manager such as [patchage](http://drobilla.net/software/patchage).

##### Overview:
```
USAGE:
    midiplex alsa [OPTIONS] <NAMES>...

OPTIONS:
    -o, --output-pool-size <O>  sets output pool size

ARGS:
    <NAMES>...                  space-delimited names of output ports
```

##### Example:
```
$ midiplex alsa melete mneme aoide & aconnect -l
[1] 10337
client 0: 'System' [type=kernel]
    0 'Timer           '
    1 'Announce        '
client 14: 'Midi Through' [type=kernel]
    0 'Midi Through Port-0'
client 128: 'midiplex' [type=user,pid=10337]
    0 'input           '
    1 'melete          '
    2 'mneme           '
    3 'aoide           '
```

##### “Help! I’m getting `ENOSPC`”:
The ALSA output mode is resource-intensive. For 𝘯 outputs, each input note _on_ and _off_ event must be copied 𝘯 times. If those 𝘯 output ports are then patched to 𝘯 synthesizers, that entails additional copying of the event. These issues can be somewhat assuaged with `--input-pool-size` and `--output-pool-size`, but for large values of 𝘯, intensive pieces may nonetheless cause MIDIplex to terminate with `ENOSPC`.

#### UDP Output
For cases where ALSA output mode is unsuitable, MIDIplex can write its output directly to a UDP socket. Note _on_ and _off_ events are encoded as three-byte datagrams, [as described by the MIDI specification](https://www.midi.org/specifications/item/table-1-summary-of-midi-message). You can receive these messages using a utility such as [MIDInet](https://github.com/jswrenn/midinet) or [qmidinet](https://qmidinet.sourceforge.io/).

##### Overview:
```
USAGE:
    midiplex udp [HOSTS]...

ARGS:
    <HOSTS>...                  space-delimited socket addresses
```

##### Example:
Assuming three reachable hosts, _melete_, _mneme_ and _aoide_:
```
$ midiplex udp melete:8336 mneme:8336 aoide:8336 & aconnect -l
[1] 13114
client 0: 'System' [type=kernel]
    0 'Timer           '
    1 'Announce        '
client 14: 'Midi Through' [type=kernel]
    0 'Midi Through Port-0'
client 128: 'midiplex' [type=user,pid=13114]
    0 'input           '
```
