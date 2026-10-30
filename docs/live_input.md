# Live input

A **console** turns buttons into light. You press a key, and an effect plays on
your fixtures; you tap a beat, and a running effect pulses with it; you hold a
key, and a parameter rises while you hold it. Consoles play live, from the
session clock, instead of from a timeline.

A console is a sequence built as you play: every action is an edit at the
current time. Playing a clip places it from now for its length, starting and
stopping a clip places it between the two, a mark goes into a collection now,
and setting an automation changes its value from now on. Its layers and operator
graph are a sequence's.

Every device that plays a console runs the same Donder runtime, so only your
button presses travel: the Preview and live E1.31 or Art-Net output receive the
same timestamped commands and render the same frames.

A console never plays on top of a sequence. Stop the sequence, then perform.

## Input devices

An `InputDevice` is something you play from: the computer keyboard, a MIDI
device such as a piano keyboard, a pad controller or a box of knobs, or any
program or app that sends OSC.
Each of its controls has a name you choose and the input it reads. A `Button`
is pressed or released; an `Axis` is a position from 0 to 1.

```text
InputDevice keyboard {
  description: "The computer keyboard.",
  driver: Keyboard,
  controls: [
    Button { name: kick, description: none, input: Key { code: KeyA } },
    Button { name: tap, description: "Tap along to the beat.", input: Key { code: Space } },
  ],
}
```

Keys use the physical key names browsers use (`KeyA`, `Digit1`, `Space`,
`ShiftLeft`, `ArrowUp`, `Numpad0`), so a binding stays on the same key whatever
your keyboard layout.

A MIDI device reads the MIDI ports whose name contains its `port`, ignoring
case, or every port when `port` is `none`. Its buttons read notes, held from
note on to note off, and control changes, held at 64 and above as a sustain
pedal is. A button reading `Notes { low, high, channel }` is a range of notes,
a piano's keyboard, where every note is its own press; a single note is a range
whose `low` is its `high`. Its axes read control changes and the pitch bend
wheel.

Every MIDI input reads one `channel`, numbered 1 to 16 as devices label them,
or every channel when `channel` is `none`. A controller that sends its pads on
channel 10 and its keys on channel 1 can then give the same note numbers to a
pad and a key.

```text
InputDevice piano {
  description: "The stage piano and its pedals.",
  driver: Midi { port: "Digital Piano" },
  controls: [
    Button { name: keys, description: "All 88 keys.", input: Notes { low: 21, high: 108, channel: none } },
    Button { name: sustain, description: none, input: ControlChange { number: 64, channel: none } },
    Axis { name: hue, description: "The first knob.", input: ControlChange { number: 21, channel: 1 } },
    Axis { name: bend, description: none, input: PitchBend { channel: none } },
  ],
}
```

Notes are MIDI note numbers, 0 to 127, with middle C at 60 and a full piano
from 21 to 108; control changes are 0 to 119. A struck note carries how hard it
was struck, its velocity. Donder connects to a device's ports when you start
performing, and to ports plugged in while you perform; the Perform bar shows the
ports it reads.

An OSC device listens for [OSC](https://opensoundcontrol.stanford.edu/) messages
sent over UDP to its `port`, on every network interface, so a phone app such as
TouchOSC, a Max or Pure Data patch, or an analysis script on another computer
can play the console. Its controls read an `Address`, matched exactly. A button
is pressed when a message's first argument is above 0 and released at 0, and a
message with no arguments presses and releases it at once, which suits marks
and fixed-length plays. An axis reads the first argument, running from `low` to
`high`, so a script can send a tempo in beats per minute or a pitch in hertz as
it is.

```text
InputDevice harmony {
  description: "Chord analysis from the listener script.",
  driver: Osc { port: 9000 },
  controls: [
    Button { name: chord_change, description: none, input: Address { path: "/chord/change" } },
    Axis { name: tempo, description: none, input: Address { path: "/tempo", low: 60.0, high: 180.0 } },
  ],
}
```

An address is a `/` and then printable characters other than spaces and OSC's
pattern characters `*?[]{}#,`. Bundles play their messages in order, stamped
when they arrive. Any computer on the network can send to the port while you
perform. Two OSC devices of a setup listen on different ports; the Perform bar
shows the ports listening, or why one could not be opened.

A device's control names are unique, and no two controls read the same input;
two MIDI controls may read the same note or control change only on different
channels.
The setup lists the input devices you have, next to its controllers:
`input_devices: [devices.keyboard]`. A console can only use devices the active
setup lists. Device files are ordinary documents: share one, and anyone with the
same hardware can copy it into their project.

## Consoles

A console has the same layers and operator graph as a sequence, plus what plays
live:

- **Live mark collections** (`LiveMarks`): start empty each session and gain a
  mark each time a `Mark` action runs. Any effect with a marks parameter can
  read one, so effects such as MarkPulse react to the beat you tap.
- **Live automations** (`LiveAutomation`): a value from 0 to 1 that `Set`
  actions write, bound as sequence automation is to parameters of the
  console's clips (`ClipParam { clip, param }`) and of the operator nodes of
  its graph (`NodeParam { node, param }`). The value maps onto each bound
  parameter's declared range, as sequence automation does, and holds until the
  next `Set`; `value` is where it starts. A parameter is bound by at most one
  automation.
- **Clips** (`ConsoleClip`): a sequence clip without a start or a duration: a
  name, a layer, a fixture or group, a scope, an effect and its parameters.
  Actions name the clip they play.
- **`always`**: clips that play for the whole session.
- **Events** (`On { event, actions }`): what a control does. The event is
  `Press { control }` or `Release { control }` of a button, or
  `Move { control }` of an axis, and its actions run in order. Any number of
  `On`s may name the same event. Pressing a button that is already held, as a
  computer key repeats, does nothing.

An action is one of:

- **`Play { clip, copies, length, values }`** plays a copy of the clip for its
  length, on top of the copies already playing: up to `copies`, 1 to 16, play
  at once, and when they all play the oldest is reused. `length` is
  `Fixed { duration }` or `Scaled { by, shortest, longest }`, where `by` is
  `Velocity` (how hard the press was; a computer key presses at 1) or
  `Automation { automation }` (a live automation's current value), mapped from
  `shortest` at 0 to `longest` at 1. The length is read once when the copy
  starts; later changes do not affect copies already playing.
- **`Start { clip, values }`** plays one copy until it is stopped. Pressing
  again while the clip plays does nothing.
- **`Stop { clip }`** stops every playing copy of the clip.
- **`Toggle { clip, values }`** starts the clip if no copy plays, and otherwise
  stops it.
- **`Mark { into }`** adds a mark to a live mark collection.
- **`Set { into }`** writes the event's value into a live automation: an axis's
  position on `Move`, the velocity on `Press`, and 0 on `Release`.

A clip is played either by `Play` actions or by `Start`, `Toggle` and `always`,
not both; `Stop` stops either. A `Move` runs only `Set` actions.

The only state a console keeps is which copies of each clip play, its live
marks and its automation values.

### Played values

The `values` of a `Play`, `Start` or `Toggle` set parameters of the copy it
starts from how it was played, so every note sounds its own way:

```text
values: [
  PlayedValue { param: level, from: Velocity },
  PlayedValue { param: position, from: Fifths },
],
```

- `Velocity`: how hard the note was struck; a computer key or a pedal plays
  at 1.
- `Pitch`: the note's place in its button's note range, from the lowest at 0
  to the highest at 1; a button of one note is at its place among all 128
  notes.
- `PitchClass`: the note's name, C at 0 up the chromatic scale to B at 11/12,
  whatever its octave.
- `Fifths`: the note's name round the circle of fifths, C at 0, G at 1/12, D
  at 2/12 and on to F at 11/12, so the notes of a chord sit close together.

The value's `0..1` maps onto the parameter's declared range, as with
automation, and the parameter is a float, int, bool, enum or curve. Values are
played only on `Press`, and the last three only from a button reading notes. A
parameter a live automation binds cannot also be played. Played from a range of
notes, `Play` is polyphonic: each note takes a copy and plays it with its own
values.

### The starter's jam console

The starter project includes `consoles/jam.data.donder`, played from
`input_devices/keyboard.data.donder`. Its `background` clip plays always: a
MarkPulse on every output that pulses on each beat of the live `beat`
collection. A plays a 0.4 s `flash` on top of up to four others, S starts the
`wash` and releasing S stops it, Space taps a beat into `beat`, and D sets the
`decay` automation to 1 on press and 0 on release, lengthening the background's
pulses while it is held.

Effects a console plays are prepared when you start performing, once, with a
pool of copies for each clip: as many as its largest `copies`, or one for a clip
that is started, toggled or always playing. An effect that loops over the marks
of a collection it reads cannot read a live collection, because a live
collection's length is not known in advance; Donder reports this when it checks
the project.

## Performing

Choose **Perform** for a console: in the project's console list, or by
right-clicking the console's document in the Explorer. Donder prepares the
console on the active setup, starts its `always` clips and starts the session
clock. The Perform bar shows the console, the elapsed time and which buttons you
hold, with toggles for live output and the Preview, and **Stop**. Live output
plays a performance on E1.31 and Art-Net controllers only; starting one stops
and releases the setup's Donder controllers.

While you perform, the console's keys go to the console before any editor
shortcut, both in the main window and in the Preview window. Keys typed into a
text field stay in the field. Releasing focus releases every held button, which
runs their `Release` events. A session lasts up to about 71 minutes, the
playback clock's range.
