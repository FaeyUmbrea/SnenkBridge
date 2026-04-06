# Parameter Configuration Format

The `params` array in .snek files (and standalone JSON config files) contains parameter objects that define how tracking input maps to VTube Studio output parameters.

## Parameter object

```json
{
  "name": "FaceAngleY",
  "func": "HeadRotY",
  "min": -30.0,
  "max": 30.0,
  "defaultValue": 0.0
}
```

### Fields

- `name` (string): VTube Studio parameter name to output to.
- `func` (string): An evalexpr expression computed from tracking input variables. Empty when `delayBuffer` is present.
- `min` (number): Minimum value for the parameter.
- `max` (number): Maximum value for the parameter.
- `defaultValue` (number): Default value when no tracking data is available.
- `delayBuffer` (object, optional): If present, this parameter reads another parameter's computed output with smoothing and delay instead of evaluating `func`. See below.

## Delay buffer parameters

Parameters can reference another parameter's computed output with smoothing and delay:

```json
{
  "name": "BodyAngleX",
  "func": "",
  "min": -10.0,
  "max": 10.0,
  "defaultValue": 0.0,
  "delayBuffer": {
    "refParam": "FaceAngleX",
    "smoothing": 2.0,
    "delayCount": 8,
    "inMin": -30.0,
    "inMax": 30.0,
    "outMin": -10.0,
    "outMax": 10.0
  }
}
```

When `delayBuffer` is present, `func` is ignored. The parameter:

1. Reads the computed output of `refParam`
2. Stores it in a ring buffer of `delayCount` frames
3. Reads the oldest value from the ring buffer
4. Maps from [inMin, inMax] to [outMin, outMax]
5. Applies exponential smoothing with the given `smoothing` factor

### Delay buffer fields

- `refParam` (string): Name of the parameter whose output to reference.
- `smoothing` (number): Exponential smoothing factor. Higher values = more smoothing.
- `delayCount` (integer): Ring buffer size in frames. Controls how many frames of delay.
- `inMin`, `inMax` (number): Input range for the referenced parameter's value.
- `outMin`, `outMax` (number): Output range after mapping.

## Available input variables

Tracking data from the iOS device is available as evalexpr variables in expressions.

### Head position and rotation

`HeadPosX`, `HeadPosY`, `HeadPosZ`, `HeadRotX`, `HeadRotY`, `HeadRotZ`

### ARKit blendshapes

All 52 ARKit face tracking blendshapes are available using PascalCase names:

**Eyes:**
`EyeBlinkLeft`, `EyeBlinkRight`, `EyeLookDownLeft`, `EyeLookDownRight`, `EyeLookInLeft`, `EyeLookInRight`, `EyeLookOutLeft`, `EyeLookOutRight`, `EyeLookUpLeft`, `EyeLookUpRight`, `EyeSquintLeft`, `EyeSquintRight`, `EyeWideLeft`, `EyeWideRight`

**Brows:**
`BrowDownLeft`, `BrowDownRight`, `BrowInnerUp`, `BrowOuterUpLeft`, `BrowOuterUpRight`

**Cheeks and nose:**
`CheekPuff`, `CheekSquintLeft`, `CheekSquintRight`, `NoseSneerLeft`, `NoseSneerRight`

**Jaw:**
`JawForward`, `JawLeft`, `JawOpen`, `JawRight`

**Mouth:**
`MouthClose`, `MouthDimpleLeft`, `MouthDimpleRight`, `MouthFrownLeft`, `MouthFrownRight`, `MouthFunnel`, `MouthLeft`, `MouthLowerDownLeft`, `MouthLowerDownRight`, `MouthPressLeft`, `MouthPressRight`, `MouthPucker`, `MouthRight`, `MouthRollLower`, `MouthRollUpper`, `MouthShrugLower`, `MouthShrugUpper`, `MouthSmileLeft`, `MouthSmileRight`, `MouthStretchLeft`, `MouthStretchRight`, `MouthUpperUpLeft`, `MouthUpperUpRight`

**Tongue:**
`TongueOut`

### Special variables

- `FaceFound`: 1.0 when a face is being tracked, 0.0 otherwise.
- `Wave{N}`: Cyclic triangle wave with period N milliseconds (e.g. `Wave10000`).
- `PingPong{N}`: Cyclic sawtooth wave with period N milliseconds (e.g. `PingPong5000`).

## evalexpr functions

Expressions can use these math functions:

`math::abs`, `math::min`, `math::max`, `math::sin`, `math::cos`, `math::floor`, `math::ceil`, `math::sqrt`, `math::pow`, `math::pi`
