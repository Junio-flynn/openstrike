# Asset credits

- `Soldier.glb` — rebuilt from the user-supplied SWAT character and rifle
  animation FBX downloads from [Mixamo](https://www.mixamo.com) (© Adobe).
  The embedded game asset includes 26 named actions covering standing,
  walking, running, crouching, sliding, jumping, firing, hit reactions, and death.
  `scripts/build-mixamo-swat.py` documents the exact source-file mapping and
  reproducibly consolidates the files into one skinned GLB. Used under the
  Mixamo license as an embedded application asset — do not extract and
  redistribute it as a standalone asset.

- `CZ_Scorpion_Viewmodel.glb` — converted from the user-supplied
  `CZ Scorpion Evo3 A1 First Person Animations` archive. The exported game
  asset contains its baked FBX skeleton, 85 required weighted joints, resized
  base-color maps, and the Equip, Fire, Idle, Inspect, Reload, Reload_Empty,
  Run, Unequip, and Walk animations. OpenStrike adds interpolated movement
  sway, look sway, and recoil around those authored clips. No license or
  author information was included in the downloaded archive; verify its
  source terms before redistributing the asset outside this project.

- `AK74M_Viewmodel.glb` — converted from the user-supplied
  `fps-ak-74m-animations` archive. The optimized game asset includes Equip,
  Fire, Idle, Reload, Reload_Empty, Reload_Variant, Run, and Walk clips. No
  license or author information was included in the archive; verify its
  source terms before redistributing it outside this project.

- `UZI_Viewmodel.glb` — converted from the user-supplied
  `uzi-2026-first-person-animations` archive by
  [1Matzh](https://skfb.ly/pwxEF), licensed CC BY 4.0. The optimized game
  asset includes Equip, Fire, Idle, Inspect, Reload, Reload_Empty, Run,
  Unequip, Walk, and aim/fire-mode clips.

- `BoltActionSniper_Viewmodel.glb` — converted from the user-supplied
  `fps-animations-sniper-rifle` archive. The embedded game asset includes its
  arm rig, PBR textures, and Idle, Walk, Reload, Reload_Empty, Fire, and
  Fire_Aimed clips. `scripts/build-sniper-viewmodel.py` documents the
  reproducible conversion. No license or author information was included in
  the archive; verify its source terms before redistributing it outside this
  project.
