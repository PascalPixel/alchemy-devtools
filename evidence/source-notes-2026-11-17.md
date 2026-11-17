# Notes taken out of Alchemy's source, 17 November 2026

Alchemy's comments are for people building with it. These said how the compiler came to emit the ROM's code, so they moved here, as they stood. The tag comments of that day, whole, are in `tbs-marks-2026-11-17.tsv` and `tla-marks-2026-11-17.tsv`.

## games/tla/include/field/actor.h:136

```c
  /// Its sprite (a struct FieldSprite) when spriteStorageKind is 1, or its list
  /// of sprites when it is 2. Code that reads one kind converts it: typed, or
  /// as a union of the two, this pointer compiles differently where a store
  /// comes between two of its reads.
```

## games/tla/include/field/flag_call.h:18

```c
/// Whether the flag is clear. Its result is a temporary of its own, so the 1
/// the test builds stays apart from a caller's own 1, which CSE would share
/// with a plain !GameFlag_Get.
```

## games/tla/include/system/dma.h:56

```c
/* Unpinning trials: an ordinary input-only channel stays in r4 across
 * adjacent transfers. A read/write early-clobber channel instead reloads
 * its address at each call. Returning that channel and chaining the first
 * two window transfers matches all 66 instructions in TLA's window loader,
 * but the twelve ROMs differ: other callers also derive nearby I/O addresses
 * from the known DMA channel. System_InstallIwramAndIrqs loses its final
 * channel-to-DISPSTAT subtraction. Those uses must be preserved together;
 * the partial return-and-chain change is not adopted. */
```

## games/tla/src/battle_effect/battle_effect_vengeance.c:960

```c
      // Set in two steps: as one giv the loop pass would turn it into a walker.
```

## games/tla/src/battle_effect/battle_effect_nirvana.c:571

```c
  LOAD_BG_PALETTE(RESOURCE_BLUE_FLAME_SHEET);
  // A block of its own: camera, savedYaw and target take their stack slots
```

## games/tla/src/field/event_commands_ropes.c:286

```c
/* VIEW(ropes): routine Trig_Atan as s16 Trig_Atan(s32 y, s32 x);
 * system/system.h has s32 Trig_Atan(s32 x, s32 y); tried as the definition's
 * s32 Trig_Atan(s32 x, s32 y), which the object already declares, Rope_LayBowed
 * moves: 1 of 120 instructions in every edition (the result leaves r0 as a
 * halfword, which only a 16-bit return type copies so). */
```

## games/tla/src/scene/champa_cliff_caves.c:774

```c
  // DEAD: each owner is stored and never read; the ROM keeps the copies of
  // the first two into sl and r8, which later stand for -1.
```

## games/tla/src/scene/aqua_rock.c:1453

```c
  // UB: viewZ is a separate member, not a second element of viewX (as the
  // members, 69 of 169 instructions differ in tla-en).
```

## games/tla/src/scene/mars_lighthouse_room_m.c:743

```c
/// The line scroll's wave across and down that the rain ledges start with the
/// rain. Called plainly, LineScroll_InitWave takes its arguments in another
/// order (16 of 237 instructions of Scene_PrepareMap differ in tla-en).
```

## games/tla/src/scene/shared/whirlwind_update_lower.h:46

```c
        /* IwramMulFX reached from IwramIrqMain rather than at its fixed
         * address: */
        /* the address is a linked word read after the cosine, as the native
         */
        /* reads it after the speed (at Iwram_MulFX it is built before). */
```

## games/tbs/include/system/dma.h:37

```c
/// Dma_Fill32 through the caller's own volatile word `cell`, so a function that
/// fills several blocks keeps one word for all of them and forms its address
/// once; the word also takes the caller's place in the frame
/// (WorldMap_InitializePerspective's sp+28, where Dma_Fill32's would be sp+16).
```

## games/tla/src/scene/magma_rock.c:1369

```c
  kind = 1; // Holds one constant: the native loads 1 before the data address.
```

