# Snenk Bridge

Receive tracking data from [tracking apps](#supported-tracking-apps) on iPhone, then modify the data
according to the configuration and send to [VTubeStudio](https://store.steampowered.com/app/1325860/VTube_Studio/) on PC.

Basically it's an alternative to [VBridger](https://store.steampowered.com/app/1898830/VBridger/).\
But **free**, **lightweight** and **open source**.

## Supported tracking apps

- [VTubeStudio](https://apps.apple.com/ru/app/vtube-studio/id1511435444) (`vts` / `vtubestudio`)
- [iFacialMocap](https://apps.apple.com/ru/app/ifacialmocap/id1489470545) / [iFacialMocapTr](https://apps.apple.com/ru/app/ifacialmocaptr/id1520971310) (`ifm` / `ifacialmocap`)

## Usage

There 2 ways to use it: CLI and UI

### UI

1. Launch `snenk_bridge_ui.exe`;
2. Set path to config file (type it or use button);
3. Type **Local** phone IP address;
4. Select your tracking application;
5. Set `face found timeout` near of tracking app (in milliseconds);
6. Press `Connect`;
7. Now you can close the window.

> [!TIP]
> To show or exit program use menu in tray

### CLI

For CLI use `snenk_bridge.exe` with launch arguments.

#### Arguments

| Command                                           | Example              | Description                                 |
| ------------------------------------------------- | -------------------- | ------------------------------------------- |
| `-c <path>`, `--config <path>`                    | `-c test.json`       | Path to JSON config                         |
| `-p <IPv4>`, `--phone-ip <IPv4>`                  | `-p "192.168.0.174"` | Phone IP address                            |
| `-t <type>`, `--tracking-client <type>`           | `-t ifm`             | [Tracking client](#supported-tracking-apps) |
| `-f <timeout>`, `--face_search_timeout <timeout>` | `-f 3000`            | Timout for face searching                   |
| `-d <delay>`, `--config-reload-delay <delay>`     | `-d 10000`           | Config reload delay                         |
| `-h `, `--help`                                   | `-h`                 | Show Help                                   |
| `-V `, `--version`                                | `-V`                 | Show Version                                |

## Transformations configuration

A JSON file that defines transformations and new parameters.

For math and logic commands, I recommend looking at [that](https://docs.rs/evalexpr/latest/evalexpr/).

There is a list of parameters sent by apps.

### Cordinates

Each value ranges from negative to positive, probably will not go out of `-45...45` range.

```
HeadRotX
HeadRotY
HeadRotZ

HeadPosX
HeadPosY
HeadPosZ
```

### BlendShapes

By default, ranged from `0` to `1`.

---

Default keys:

```
BrowDownLeft
BrowDownRight
BrowInnerUp
BrowOuterUpLeft
BrowOuterUpRight

CheekPuff
CheekSquintLeft
CheekSquintRight

EyeBlinkLeft
EyeBlinkRight
EyeLookDownLeft
EyeLookDownRight
EyeLookInLeft
EyeLookInRight
EyeLookOutLeft
EyeLookOutRight
EyeLookUpLeft
EyeLookUpRight
EyeSquintLeft
EyeSquintRight
EyeWideLeft
EyeWideRight

JawForward
JawLeft
JawOpen
JawRight

MouthClose
MouthDimpleLeft
MouthDimpleRight
MouthFrownLeft
MouthFrownRight
MouthFunnel
MouthLeft
MouthLowerDownLeft
MouthLowerDownRight
MouthPressLeft
MouthPressRight
MouthPucker
MouthRight
MouthRollLower
MouthRollUpper
MouthShrugLower
MouthShrugUpper
MouthSmileLeft
MouthSmileRight
MouthStretchLeft
MouthStretchRight
MouthUpperUpLeft
MouthUpperUpRight

NoseSneerLeft
NoseSneerRight

TongueOut
```

---

Exclusive for `iFacialMocap`:

```
Hapihapi
```

---

Exclusive variables for this bridge:

```python
# Indicates whether a face was found.
FaceFound
# In one second (1000ms), its value changes linearly from 0 to 1 and back.
Wave1000
# Linearly changes its value from 0 to 1 per 5 seconds (5000ms). When it reaches 1, it resets to 0.
PingPong5000
```

Activated when a face is first detected.

### Example

```json
[
  {
    "name": "FaceAngleY",
    "func": "- HeadRotY * 1",
    "min": -40.0,
    "max": 40.0,
    "defaultValue": 0
  },
  {
    "name": "FaceAngleX",
    "func": "(((HeadRotX * ((90 - math::abs(HeadRotY)) / 90)) + (HeadRotZ * (HeadRotY / 45))))",
    "min": -40.0,
    "max": 40.0,
    "defaultValue": 0
  },
  {
    "name": "FaceAngleZ",
    "func": "((HeadRotZ * ((90 - math::abs(HeadRotY)) / 90)) - (HeadRotX * (HeadRotY / 45)))",
    "min": -40.0,
    "max": 40.0,
    "defaultValue": 0
  },
  {
    "name": "FacePositionX",
    "func": "HeadPosX * - 1",
    "min": -15.0,
    "max": 15.0,
    "defaultValue": 0
  },
  {
    "name": "FacePositionY",
    "func": "HeadPosY",
    "min": -15.0,
    "max": 15.0,
    "defaultValue": 0
  },
  {
    "name": "FacePositionZ",
    "func": "HeadPosZ",
    "min": -15.0,
    "max": 15.0,
    "defaultValue": 0
  },
  {
    "name": "MouthOpen",
    "func": "(((JawOpen - MouthClose) - ((MouthRollUpper + MouthRollLower) * .2) + (MouthFunnel * .2)))",
    "min": 0.0,
    "max": 1.0,
    "defaultValue": 0
  },
  {
    "name": "EyeRightX",
    "func": "(EyeLookInLeft - .1) - EyeLookOutLeft",
    "min": -1.0,
    "max": 1.0,
    "defaultValue": 0
  },
  {
    "name": "EyeRightY",
    "func": "(EyeLookUpLeft - EyeLookDownLeft) + (BrowOuterUpLeft * .15) + (HeadRotX / 30)",
    "min": -1.0,
    "max": 1.0,
    "defaultValue": 0
  },
  {
    "name": "EyeOpenLeft",
    "func": ".5 + ((EyeBlinkLeft * - .8) + (EyeWideLeft * .8))",
    "min": 0.0,
    "max": 1.0,
    "defaultValue": 0
  },
  {
    "name": "EyeOpenRight",
    "func": ".5 + ((EyeBlinkRight * - .8) + (EyeWideRight * .8))",
    "min": 0.0,
    "max": 1.0,
    "defaultValue": 0
  },
  {
    "name": "MouthSmile",
    "func": "(2 - ((MouthFrownLeft + MouthFrownRight + MouthPucker) / 1) + ((MouthSmileRight + MouthSmileLeft + ((MouthDimpleLeft + MouthDimpleRight) / 2)) / 1)) / 4",
    "min": -1.0,
    "max": 1.0,
    "defaultValue": 0
  },
  {
    "name": "EyeSquintL",
    "func": "EyeSquintLeft",
    "min": 0.0,
    "max": 1.0,
    "defaultValue": 0
  },
  {
    "name": "EyeSquintR",
    "func": "EyeSquintRight",
    "min": 0.0,
    "max": 1.0,
    "defaultValue": 0
  },
  {
    "name": "MouthX",
    "func": "(((MouthLeft - MouthRight) + (MouthSmileLeft - MouthSmileRight)) * (1 - TongueOut))",
    "min": -1.0,
    "max": 1.0,
    "defaultValue": 0
  },
  {
    "name": "CheekPuff",
    "func": "CheekPuff",
    "min": 0.0,
    "max": 1.0,
    "defaultValue": 0
  },
  {
    "name": "TongueOut",
    "func": "TongueOut",
    "min": 0.0,
    "max": 1.0,
    "defaultValue": 0
  },
  {
    "name": "MouthPucker",
    "func": "(((MouthDimpleRight + MouthDimpleLeft) * 2) - MouthPucker) * (1 - TongueOut)",
    "min": -1.0,
    "max": 1.0,
    "defaultValue": 0
  },
  {
    "name": "MouthFunnel",
    "func": "(MouthFunnel * (1 - TongueOut)) - (JawOpen * .2)",
    "min": 0.0,
    "max": 1.0,
    "defaultValue": 0
  },
  {
    "name": "JawOpen",
    "func": "JawOpen",
    "min": 0.0,
    "max": 1.0,
    "defaultValue": 0
  },
  {
    "name": "MouthPressLipOpen",
    "func": "(((MouthUpperUpRight + MouthUpperUpLeft + MouthLowerDownRight + MouthLowerDownLeft) / 1.8) - (MouthRollLower + MouthRollUpper)) * (1 - TongueOut)",
    "min": -1.3,
    "max": 1.3,
    "defaultValue": 0
  },
  {
    "name": "MouthShrug",
    "func": "((MouthShrugUpper + MouthShrugLower + MouthPressRight + MouthPressLeft) / 4) * (1 - TongueOut)",
    "min": 0.0,
    "max": 1.0,
    "defaultValue": 0
  },
  {
    "name": "BrowInnerUp",
    "func": "BrowInnerUp",
    "min": 0.0,
    "max": 1.0,
    "defaultValue": 0
  },
  {
    "name": "BrowLeftY",
    "func": ".5 + (BrowOuterUpLeft - BrowDownLeft) + ((MouthRight - MouthLeft) / 8)",
    "min": 0.0,
    "max": 1.0,
    "defaultValue": 0
  },
  {
    "name": "BrowRightY",
    "func": ".5 + (BrowOuterUpRight - BrowDownRight) + ((MouthLeft - MouthRight) / 8)",
    "min": 0.0,
    "max": 1.0,
    "defaultValue": 0
  },
  {
    "name": "Brows",
    "func": ".5 + (BrowOuterUpRight + BrowOuterUpLeft - BrowDownLeft - BrowDownRight) / 4",
    "min": 0.0,
    "max": 1.0,
    "defaultValue": 0.5
  },
  {
    "name": "VoiceFrequencyPlusMouthSmile",
    "func": "(2 - ((MouthFrownLeft + MouthFrownRight + MouthPucker) / 1) + ((MouthSmileRight + MouthSmileLeft + ((MouthDimpleLeft + MouthDimpleRight) / 2)) / 1)) / 4",
    "min": 0.0,
    "max": 1.0,
    "defaultValue": 0.5
  },
  {
    "name": "BodyAngleX",
    "func": "- HeadRotY * 1.5",
    "min": -40.0,
    "max": 40.0,
    "defaultValue": 0
  },
  {
    "name": "BodyAngleY",
    "func": "( - HeadRotX * 1.5)  + ( (EyeBlinkLeft + EyeBlinkRight) * - 1)",
    "min": -40.0,
    "max": 40.0,
    "defaultValue": 0
  },
  {
    "name": "BodyAngleZ",
    "func": "HeadRotZ * 1.5",
    "min": -40.0,
    "max": 40.0,
    "defaultValue": 0
  },
  {
    "name": "BodyPositionX",
    "func": "HeadPosX * - 1",
    "min": -15.0,
    "max": 15.0,
    "defaultValue": 0
  },
  {
    "name": "BodyPositionY",
    "func": "HeadPosY * 1",
    "min": -15.0,
    "max": 15.0,
    "defaultValue": 0
  },
  {
    "name": "BodyPositionZ",
    "func": "HeadPosZ * - .5",
    "min": -15.0,
    "max": 15.0,
    "defaultValue": 0
  },
    {
    "name": "Wave",
    "func": "Wave10000 * FaceFound",
    "min": 0.0,
    "max": 1.0,
    "defaultValue": 1
  },
  {
    "name": "PingPong",
    "func": "PingPong5000",
    "min": 0.0,
    "max": 1.0,
    "defaultValue": 0
  }
]
```
