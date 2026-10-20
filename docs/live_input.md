# Live input

A **rig** turns buttons into light. You press a key, and an effect plays on your
fixtures; you tap a beat, and a running effect pulses with it; you hold a key,
and a parameter rises while you hold it. Rigs play live, from the session clock,
instead of from a timeline.

Every device that plays a rig runs the same Donder runtime, so only your button
presses travel: the Preview and live E1.31 or Art-Net output receive the same
timestamped commands and render the same frames.

A rig never plays on top of a sequence. Stop the sequence, then perform.

## Input devices

An `InputDevice` is a piece of hardware you play from: the computer keyboard,
or a MIDI device such as a piano keyboard, a pad controller or a box of knobs.
Each of its controls has a name you choose and the input it reads. A `Button`
is pressed or released; a `Dial` is a position from 0 to 1.

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
pedal is. Its dials read control changes and the pitch bend wheel. Every
channel plays.

```text
InputDevice piano {
  description: "The stage piano and its pedals.",
  driver: Midi { port: "Digital Piano" },
  controls: [
    Button { name: middle_c, description: none, input: Note { note: 60 } },
    Button { name: sustain, description: none, input: ControlChange { number: 64 } },
    Dial { name: hue, description: "The first knob.", input: ControlChange { number: 21 } },
    Dial { name: bend, description: none, input: PitchBend },
  ],
}
```

Notes are MIDI note numbers, 0 to 127, with middle C at 60; control changes are
0 to 119. Donder connects to a device's ports when you start performing, and to
ports plugged in while you perform; the Perform bar shows the ports it reads.

A device's control names are unique, and no two controls read the same input.
The setup lists the input devices you have, next to its controllers:
`input_devices: [devices.keyboard]`. A rig can only use devices the active setup
lists. Device files are ordinary documents: share one, and anyone with the same
hardware can copy it into their project.

## Rigs

A rig has the same layers and operator graph as a sequence, plus what plays live:

- **Live mark collections** (`LiveMarks`): start empty each session and gain a
  mark each time a `Mark` mapping fires. Any effect with a marks parameter can
  read one, so effects such as MarkPulse react to the beat you tap.
- **Scene voices** (`Voice`): effects that play for the whole session, like a
  clip without a start or an end: a layer, a fixture or group, a scope, an effect
  and its parameters.
- **Switches** (`Switch`): an on/off state that each press of its button flips.
  Switches start off. Use them as pages: the same key does one thing while a
  switch is off and another while it is on.
- **Mappings** (`Mapping`): what a button does. A mapping's `when` makes it act
  only while a switch is `On` or `Off`; `none` acts always.

A mapping's action is one of:

- **Trigger** starts an effect: a layer, target, scope, effect and parameters,
  like a clip. Its `length` is `Fixed { duration }` (plays that long from each
  press), `Held` (plays while you hold the button) or `Latched` (one press starts
  it, the next stops it). Its `voices` decide what another press does while it
  plays: `Retrigger` restarts it, `Stack { count }` starts another copy on top,
  up to `count`, reusing the oldest, and `Mono` ignores the press.
- **Mark** adds a mark to a live collection.
- **Control** holds a parameter of a scene voice or of a trigger's voices at the
  top of its range while the button is held, and at the bottom otherwise. Played
  from a dial, it follows the dial anywhere in that range. The range is the
  parameter's declared range, as with automation. Speeds and other values the
  effect integrates change smoothly, without jumping.

Switches are played from buttons. A dial can only play a `Control` mapping.

The starter project includes `rigs/jam.data.donder`, played from
`input_devices/keyboard.data.donder`: A flashes the outputs (or bursts them while
Shift's switch is on), S washes them while held, D lengthens the background's
pulses while held, and
Space taps the beat the background pulses to.

Effects a rig plays are prepared when you start performing, once, with a pool of
voices for each trigger. An effect that loops over the marks of a collection it
reads cannot read a live collection, because a live collection's length is not
known in advance; Donder reports this when it checks the project.

## Performing

Choose **Perform** for a rig: in the project's rig list, or by right-clicking the
rig's document in the Explorer. Donder prepares the rig on the active setup and
starts the session clock. The Perform bar shows the rig, the elapsed time, which
switches are on and which buttons you hold, with toggles for live output and the
Preview, and **Stop**.

While you perform, the rig's keys go to the rig before any editor shortcut, both
in the main window and in the Preview window. Keys typed into a text field stay
in the field. Releasing focus releases every held button. A session lasts up to
about 71 minutes, the playback clock's range.
