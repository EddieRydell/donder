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
is pressed or released; a `Dial` is a position from 0 to 1; `Keys` are a range
of MIDI notes, a piano's keyboard, where every key is its own press.

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
    Keys { name: keys, description: "All 88 keys.", input: Notes { low: 21, high: 108 } },
    Button { name: sustain, description: none, input: ControlChange { number: 64 } },
    Dial { name: hue, description: "The first knob.", input: ControlChange { number: 21 } },
    Dial { name: bend, description: none, input: PitchBend },
  ],
}
```

Notes are MIDI note numbers, 0 to 127, with middle C at 60 and a full piano
from 21 to 108; control changes are 0 to 119. A struck note carries how hard it
was struck, its velocity. Donder connects to a device's ports when you start performing, and to
ports plugged in while you perform; the Perform bar shows the ports it reads.

### Listeners

A listener hears the music played on MIDI ports and turns it into controls, so
a rig can answer the music itself rather than single keys. Its buttons are
moments in the music and its dials running measures of it:

```text
InputDevice ear {
  description: "Listens to the piano.",
  driver: Listen { port: "Digital Piano" },
  controls: [
    Button { name: chord_change, description: none, input: ChordChange },
    Button { name: home, description: "V resolves to I.", input: Resolution },
    Button { name: beat, description: none, input: Beat },
    Button { name: minor, description: none, input: Chord { quality: Minor } },
    Button { name: nobody, description: none, input: Idle { seconds: 30 } },
    Dial { name: key_color, description: none, input: KeyColor },
    Dial { name: tension, description: none, input: Tension },
  ],
}
```

| Button | Pressed |
| --- | --- |
| `ChordChange` | when the chord changes |
| `KeyChange` | when the key changes, once the new key has led for a moment |
| `Resolution` | when a dominant seventh, or a major chord in the key, falls a fifth to its home chord: V to I |
| `Beat` | on each beat at the player's tempo, between notes too, until a few seconds after they stop |
| `RunUp`, `RunDown` | at a fast run of six or more notes spanning at least a fifth |
| `BigChord` | when five or more notes are struck hard together |
| `Chord { quality }` | held while the chord is `Major`, `Minor`, `Dominant`, `Diminished`, `Augmented` or `Suspended` |
| `Mode { mode }` | held while the key is `Major` or `Minor` |
| `Idle { seconds }` | held once nothing has been played for that long, until someone plays |
| `Tune { notes }` | when the melody is played, in any key and any octave |

A tune's `notes` are the MIDI notes of a melody's opening, 5 to 24 of them, in
any key: the listener follows the top note of what is played above G3 and
hears the tune when the last notes fit it in one key, forgiving about one wrong
note in six. The chorus of Jingle Bells is
`Tune { notes: [64, 64, 64, 64, 64, 64, 64, 67, 60, 62, 64] }`; the editor
writes them as note names, `E4 E4 E4 E4 E4 E4 E4 G4 C4 D4 E4`.

The first seven, and tunes, are momentary: pressed and released at once, so they play
`Fixed` triggers, marks and switches.

| Dial | From 0 to 1 |
| --- | --- |
| `KeyColor` | the key's tonic round the circle of fifths: C at 0, G at 1/12, D at 2/12 |
| `ChordColor` | the chord's root round the circle of fifths |
| `Tension` | low on the home chord, high on dominants, diminished chords and notes outside the key |
| `Loudness` | how hard the music is played, falling away in silence |
| `Density` | notes a second, from none to twelve |
| `Register` | how high the sounding notes are, from the bottom of a piano to its top |
| `Tempo` | the player's tempo, from 60 beats a minute to 200 |

A listener names chords from the notes sounding, counting the sustain pedal,
how hard and how recently each note was struck, and the bass note; melody
notes above C5 count less, and lifting notes never changes the chord. It finds
the key from the notes of the last several seconds, naming the first one
after about sixteen notes and a new one once it has led for a few seconds;
`KeyChange` waits until the old key has held for eight seconds, so finding
the first key is not a change. It finds the tempo from the intervals between
notes. Its controls play rigs like any other device's.

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
  like a clip. Its `values` set parameters of each voice it starts from how it
  was played (below). Its `length` is `Fixed { duration }` (plays that long from each
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

Switches are played from buttons. A dial can only play a `Control` mapping, and
keys play `Trigger` and `Mark` mappings.

### Played values

A trigger's `values` set parameters of the voice each press starts, so every
note sounds its own way:

```text
values: [
  PlayedValue { param: level, from: Velocity },
  PlayedValue { param: position, from: Fifths },
],
```

- `Velocity`: how hard the note was struck; a computer key or a pedal plays
  at 1.
- `Pitch`: the note's place in its keys' range, from the lowest at 0 to the
  highest at 1; a single note button is at its place among all 128 notes.
- `PitchClass`: the note's name, C at 0 up the chromatic scale to B at 11/12,
  whatever its octave.
- `Fifths`: the note's name round the circle of fifths, C at 0, G at 1/12, D
  at 2/12 and on to F at 11/12, so the notes of a chord sit close together.

The value's `0..1` maps onto the parameter's declared range, as with
automation, and the parameter is a float, int, bool, enum or curve. Played
from keys, a `Stack` trigger is polyphonic: each key takes a voice, plays it
with its own values, and with a `Held` length stops it when released.

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
