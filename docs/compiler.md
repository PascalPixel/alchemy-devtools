# How the old compiler decides

A study of alchemy-gcc, the compiler both games build with, written from what
two rounds of work on the hardest fake matches found (15 and 16 November
2026) and checked again here. It is for whoever next asks why a routine will
not match as it is plainly written.

The compiler is GNU's GCC snapshot of 31 July 2000 for Thumb, at `-O2`
(`alchemy-gcc/gcc`). Every decision below is that compiler's own code, and
every file and function named is in that folder.

Each finding carries one of three marks:

- **Reproduced**: a small program was compiled with the build's own compiler
  and options for this study, and the result is quoted.
- **Read**: the condition was found in the compiler's source at the place
  named, and a round measured its effect in the games.
- **Reported**: a round measured or inferred it; not checked again here.

To reproduce one: preprocess with `tools/out/compilers/cpp0 -nostdinc`, then
`tools/out/compilers/cc1 FILE.i -quiet -O2 -mthumb -mthumb-interwork
-fcall-used-r4 -o FILE.s`. Add `-da` for a dump after every pass.

## The passes, in order

A routine goes through these, and `-da` leaves one dump for each:

| Dump | Pass | What it decides here |
| --- | --- | --- |
| `rtl` | expand | the first instructions, loops rotated, calls and their arguments |
| `jump` | jump | loop tests copied ahead, jumps threaded |
| `cse` | first CSE | values shared along one path |
| `addressof` | addressof | which locals live in the frame |
| `gcse` | global CSE | values shared across blocks, new registers numbered |
| `loop` | loop | what moves out of a loop, counters reversed, empty loops |
| `cse2` | second CSE | values shared again, jumps threaded again |
| `cfg`, `life` | flow | dead code removed, references weighed by loop depth |
| `combine` | combine | instructions joined within a block |
| `ce` | if-conversion | a short arm lifted above its test |
| `lreg`, `greg` | allocation | which register each value gets, and reload |
| `flow2`, `ce2` | flow again | |
| `sched2` | scheduling | the order of instructions within a block |
| `jump2` | jump again | identical tails joined |
| `mach` | `arm_reorg` | where the literal pools go |

The order explains a good deal by itself. Tails are joined only at the end,
so a tail written twice still shifts everything allocation and scheduling do
before it. No jump pass runs between `life` and `jump2`, so a branch whose
arm turns out empty is still a block boundary through allocation and
scheduling.

## Expand

**A call to a routine declared to return a value orders its arguments
otherwise. Reproduced.** The call ends in a use of r0, which gives the copy
into r0 one more dependent when the scheduler breaks a tie.

```c
void PaletteFade_SetTarget(int, int);  /* or: int PaletteFade_SetTarget(int, int); */
void f(void) { PaletteFade_SetTarget(0x10000, 0); }
```

Declared `void`: `mov r0, #128; lsl r0, r0, #9; mov r1, #0`. Declared `int`:
`mov r0, #128; mov r1, #0; lsl r0, r0, #9`. A call with no prototype in scope
is a value call, so a missing include does the same. This sourced
`MenuArrow_SetPosition`, Dehkan's tightropes and `Islets_StartWaveDma`.

**A loop is rotated around its first exit, if that exit is late. Read
(`stmt.c`, `expand_end_loop`: `if (last_test_insn && num_insns > 30) break`).**
If no jump to the loop's end lies in its first 30 instructions, the first
such jump anywhere becomes the exit test and everything before it moves
below the body. Only `break` and a `for` or `while` test count; a `goto` out
does not. This is why some retry loops match only as
`for (;;) { ...continue...; break; }`, and why a loop with a late `break`
compiles as the `do`-`while` it is.

**A narrow parameter of an inline is made before the call. Reported.** With
`u16 facing` on `Actor_SetPositionAndFacing` the constant is built first and
later turns share its register. 97 locals that stood in for this remain in 26
scenes of The Lost Age; in a probe 12 of 19 scenes matched at once without
them. It is not general: the same change on `Event_GetFlag` moves 129 lines
in one scene.

**Narrowing in fold. Reported.** A mask or shift read from a halfword itself
becomes a byte or halfword operation whose constant is then shared
(`RGB_RED(*dst)`). A constant pointer is a sum's second operand by RTL's
canonical order, so `Ram_MapCellBuffer + picture * 6000` is plain; this
corrects the first round, which put it down to `pointer_int_sum`.

**The order of a sum's operands. Read (`expr.c`, `expand_expr`,
`both_summands`; `optabs.c`, `expand_binop`) and measured, 19 November
2026.** Two rules decide which of a pointer and its index comes first, and
the order reaches the bytes as the operand order of an `add` or of a
register-indexed load.

In an address (a memory access, expanded with `EXPAND_SUM`) a sum with a
constant is put last and a multiplication first. So `p[i].member`, whose
offset is one tree `i * size + offset`, is base + (i × size + offset), and
`(p + i)->member` is (base + i × size) + offset:

```c
return work->entries[index].flags;     /* lsl r0, #4; add r0, #24; ldrsh r0, [r3, r0] */
return (work->entries + index)->flags; /* lsl r0, #4; add r0, r0, r3; mov r3, #24; ldrsh r0, [r0, r3] */
```

In a value (a sum assigned or passed) `expand_binop` keeps the source's
order, pointer first, but swaps the two when the first is not a register and
the second is: a global's address or a member read from memory is not one
yet, a local pointer is. So `gMaps + map` is (map × size, gMaps) and
`maps + map`, with `maps` a local, is (maps, map × size).

This is the whole of "the address summed as an integer, its offset first":
the cast to an integer only puts the index first. Where the pointer is a
global the plain sum already does. Where the ROM also keeps that pointer in
one register for the routine, the integer local is doing two things and no
pointer form does both.

**A store to a bit-field reads and writes the smallest unit that holds it,
and builds its mask negated. Measured, 19 November 2026.**
`sprite->priority = 1`, with `priority` two bits of a halfword, compiles in
`FieldEffect_SetAttached` to `ldrb r3, [r6, #9]; mov r2, #13; neg r2, r2;
and r2, r2, r3; mov r3, #4; orr r2, r2, r3; strb r2, [r6, #9]`. A device
that masks a byte by hand with `-(mask + 1)` is that store written out.

## Jump, and the loop's test

**A rotated loop's test is copied ahead of the loop. Read (`jump.c`,
`duplicate_loop_exit_test`).** Unless the test holds a label, a call or a
loop note. With constant bounds the copy folds away. An entry jump to a
bottom test in the ROM is therefore either such a test, or the copy joined
again by `jump2`.

**The loop pass moves invariants out on a budget, in the loop's own order.
Read in `loop.c` and the dumps (`DjinnMenu_ShowMemberStatus`, The Lost Age,
17 November 2026).** `move_movables` walks the values a loop never changes
in the order their instructions stand in the loop, and moves one ahead of
the loop when `threshold * savings * lifetime >= instructions in the loop`.
`threshold` starts at one more than the number of registers that are not
fixed (twice that in a loop with no call), and every move takes 3 from it. So the first few
invariants in a long loop move and the later, short-lived ones do not. The
pass runs twice at `-O2`; what the second run moves is placed after what
the first moved. Reload then deals its registers in that order, so the
order shows in the bytes even where every instruction is otherwise the same.

In this routine the ROM sets up, ahead of the loop, a pointer to the page
array, then the count array's address, then `pairMember`'s address. With
both arrays indexed directly, as The Broken Seal's twin is written,
`pairMember`'s address is the first invariant the loop reaches and moves in
the first run; the two array addresses turn up at the page turn, far down
the loop, and land after it: one block of eleven instructions with four
registers rotated. The ROM's order needs two invariants ahead of
`pairMember` at the loop's top (with the zero both share, three moves,
which spends the budget and leaves `pairMember` for the second run).

**A declaration's initialiser takes its registers before the next
declaration does, and stack slots go out in register order.** A local is
given its register where it is declared, and an initialiser is expanded
there too, so `s32 *a = &x[i]; s32 *b = &y[i];` numbers the address of `x`
before `b`. Reload hands stack slots to the values left without a register
in number order. In the same routine the ROM keeps the page array's address
in the slot above the count pointer's; assigned after both declarations
(`s32 *a; s32 *b; a = &x[i];`) the two slots swap, 12 lines. The routine
had carried `selected = selectedPages` to force that order; with the two
pointers declared with their initialisers at the top of the loop's block,
in the ROM's order, it matches without it. Swapped, 18 lines differ.

What that left: the routine went from four tagged pointers to one
(`counts = pageCounts`, now set once before the loop; without it the loop's
first read of the count folds into the count pointer and loses its index
register, one instruction). The twin in The Broken Seal turned out not to
need its `counts` pointer at all. When a loop's set-up block is rotated
against the ROM, read the loop dump's `moved to` lines of both forms before
trying stand-ins: they say which value has to reach the loop's top first.

**A reversed loop sets its counter after what was moved out of it.
Measured, 17 November 2026.** When the loop pass reverses an upward loop
whose body never reads the counter, it writes the counter's new start just
ahead of the loop, after the invariants it has already moved there. A loop
written as a countdown has its start in the source, ahead of them. So a
ROM that takes a local's address and then sets a countdown's start had an
upward loop in its source. Six routines in The Lost Age carried a pointer
to a local, set just above a countdown, to get that order; written upward
(`for (n = 0; n < 5; n++)`) they need none. Twenty-six more countdowns of
the kind, with nothing leaning on them, compile the same upward in every
edition. A pointer that only stands for a local and is set right above a
countdown is the sign to look for.

**Jump threading stops at any instruction between two tests. Read (`jump.c`,
`thread_jumps`).** It runs after each CSE. A store between two blocks that
test one condition keeps them apart, but only a store to a variable used
elsewhere, since any other is deleted first. The plain form this gave: an
`if` block closed and reopened on one condition.

## The first CSE

**It forgets everything after 1,000 instructions on one path. Reproduced
(`cse.c`, `cse_basic_block`: `if (code != NOTE && num_insns++ > 1000)
flush_hash_table ()`).** The source's own comment calls it "a real kludge".

```c
extern int q[];
int f(int a, int b, int *p) {
  p[0] = a * b;
  q[1] = 8; q[2] = 15; /* ... N such stores ... */
  return a * b;
}
```

With 277 stores the routine is 1,002 instructions at the second CSE and the
product is computed once. With 278 it is 1,006 and computed twice. The first
CSE shares it in neither case, since the path is longer there. A loop note
ends a path and starts the count again.

This is the likely cause of the empty once-only blocks in very long event
routines (the lobby, both Mars scenes, Airs Rock, Dehkan, the status
screen), which the first round put down to scheduling: the block stands where
the flush fell in the original. A routine's blocks should be removed
together, counting instructions from the last label.

**What ends its path. Reported.** An if-else join or a loop note. When a loop
or statement macro is an arm's last statement, its loop-end note lands
between the arm's barrier and the else label, and the scan stops there.

**A local whose address is taken is hidden from the first CSE only.**
See addressof, below.

## Addressof: which locals live in the frame

**A scalar local stays in memory in two cases only: it is `volatile`, or its
address is taken and it is narrower than a word. Reproduced (`function.c`,
`put_var_into_stack`: `can_use_addressof` needs `decl_mode ==
promoted_mode`, under "FIXME make it work for promoted modes too").**

```c
extern volatile unsigned short REG;
void g(void);
T h(void) { T x; *&x = REG; g(); return x; }
```

With `T` an `unsigned short`: `strh r3, [r5]` before the call and
`ldrh r0, [r5]` after, in a 4-byte frame. With `unsigned int`:
`ldrh r5, [r3]`, the call, `mov r0, r5`, and no frame. This is the source of
every stack halfword in both games (`*&x`, `Io_Read16`, `Io_Write8`).

## Global CSE

**The size of its hash table numbers the new registers, and reload hands out
stack slots in that order. Read (`gcse.c`, `alloc_expr_hash_table`:
`expr_hash_table_size = n_insns / 2`, made odd).** So one instruction more
before this pass can swap two spill slots and nothing else. The devices that
rest on it (Iris's 274 unused locals, Light Surge, `BattleItemMenu_Run`) only
steer a numbering. There is no source form to search for.

**It gives up when two computations of a value reach. Reported
(`computing_insn`).** A pointer set to a frame address twice in a loop is
replaced by neither. Holds the three ice slides.

**Partial redundancy puts a value on every path into a join. Reported.** A
local with a label pair such as `X` and `X_scripted` is the compiler's own
insertion, not source: `Battle_Run` builds the same with the test written at
each use.

## Loop

**A loop made of labels has no loop notes. Read.** So this pass and `jump`
leave it alone. But the notes matter even to an ignored loop, because flow
weighs references by loop depth: that alone swaps two registers in the debug
browsers. `while (1)` and `for (;;)` do not compile alike.

**Which loops it refuses. Read (`loop.c`: "is phony", "ignored due to
multiple entry points").** A loop entered by a jump to its test, a
`do { } while (0)` block, and a loop whose body opens with
`if (c) { statements; break; }`, which gets a second entry when its test is
copied ahead. A plain `if (c) break;` does not do it.

**A counter the body does not read is reversed, but only for a `<` test.
Reproduced (`check_dbra_loop`).**

```c
for (i = 0; i < 10; i++) g();   /* mov r5, #9; sub r5, #1; bl g; cmp r5, #0; bge */
for (i = 0; i != 10; i++) g();  /* mov r5, #0; add r5, #1; bl g; cmp r5, #10; bne */
```

So the downward `for (i = N - 1; i >= 0; i--)` loops in the source, where the
body never reads `i`, are upward loops written as the compiler left them.

**Empty loops. Reproduced.** Counted in an `int`, an empty loop of 256 passes
or fewer is deleted outright, and one of 257 is kept as `sub; cmp; bne`.
Counted in a `short`, it is kept and reversed for any count up to 256
(`mov r3, #N-1; sub r3, #1; cmp r3, #0; bge`); from 257 it is kept but no
longer reversed. The rounds had "kept at any count" for the short counter;
the limit of 256 is new here. An empty loop in the ROM therefore says how its
counter was declared, and a loop whose body only reads is emptied later and
leaves its count and branch behind.

**What moves out of a loop. Read (`move_movables`: `threshold * savings *
lifetime >= insn_count`).** The threshold is 15 with a call in the loop. The
pass runs twice, so one address can move in the first and another in the
second. Several devices turn on a life of 25 against 26 instructions. A set
that may never run is not moved unless its register is confined to one block,
which is why an inline's own local is hoisted and a function-level variable
is not.

**A volatile reference stops strength reduction. Reported.** A `(u16 *)` cast
on `BG_PLTT` hid the volatile and made a loop reducible, which is why such
loops had been written with labels. 17 sites in The Lost Age still cast.

## If-conversion

**Thumb's branch cost is 1. Read (`config/arm/arm.h`: `BRANCH_COST` is
`optimize > 1 ? 1 : 0` for Thumb).** A one-instruction early-out is lifted
above its test only if what it sets is dead on the other side. Otherwise the
fall-through block is lifted, if it has one way in and one out and touches no
memory. So `return NULL` on an early-out lets a loop's set-up be lifted, and
a result variable does not. This removed two barriers in the rope searches.

## Allocation and reload

**Rank is `floor_log2(refs) × refs / live length`. Read.** A local whose
value is computed where it is declared lives longer and can lose its
register to another. An early `return x;` is one more reference, which takes
the product from 3 to 8 and was the source form for one register pin.

**One variable assigned twice is one web. Reported.** The whole of it takes
one callee-saved register if any part crosses a call. About eight devices
rest on this and need a ruling, not a search.

**Reload's spill registers rotate over the whole routine. Read (`reload1.c`,
`last_spill_reg`).** Writing a shared tail out twice shifts every later
reload, although the copies are joined afterwards.

**Devices hold one another up. Reported.** Each stand-in pointer is a
register born where it is set. Measured alone, removing one costs lines;
removing all of a routine's together can cost none. The evidence tables'
figures were all measured one at a time, so some are stale.

**Its own CSE is emptied by a label, a call or an asm. Reported.** A value
the ROM reloads from the stack though a register still holds it means a
label stood there.

## Scheduling

**Ties are broken by priority, then relation to the last instruction placed,
then the count of dependents, then original order. Read (`haifa-sched.c`,
`rank_for_schedule`).** A loop note makes the next instruction depend on
everything before it. An empty asm is a barrier of its own. This is the
largest family of devices left.

**Volatile reads and writes are ordered against each other. Reported.**
`Display_ApplyBg2Affine`'s barrier went once four members of the work struct
were declared volatile. The first round had proposed changing the compiler's
load timing for this site; that is no longer needed.

**Alias sets decide which store may sink past a load. Reported.** A member
narrower than a word takes the set of its outermost struct; a word member
takes its own type's; a `u8` array element takes set 0, which may alias
anything.

## Jump again

**Which identical tails are joined. Reported.** Two copies that both end in a
jump to one label are joined, calls included, and the later copy is kept. A
copy that falls into the label is joined only if the tail does not end in a
call.

## Literal pools

**An asm statement counts as 4 bytes, whatever it emits. Reproduced
(`config/arm/arm.c`, `create_fix_barrier`).** Pools are placed from estimated
lengths.

```c
void g(int);
void f(unsigned short *h, int *p) {
  *h = 0x1234;
  p[0] = 1; /* ... */ p[12] = 13;
  __asm__("");        /* with or without this line */
  p[13] = 14; p[14] = 15;
  g(1);
}
```

Without the asm the pool follows `str r3, [r1, #52]`. With it the pool comes
one store earlier, after `str r3, [r1, #48]`, though the asm emits nothing.
Nine barriers in both games rest on exactly these 4 bytes, all after
`sprite.attr01 = SHAPE; sprite.attr23 = 0;`. A replay over 41,547 pools of
The Lost Age placed every one right with this rule.

## What this means for the devices left

| Family | Decided by | A source form? |
| --- | --- | --- |
| Facing locals (97) | expand | yes: `u16 facing` on the inline, already made |
| Routines with several tags | allocation | often: remove them together and measure again |
| Once-only blocks in long routines | first CSE's flush | likely: remove a routine's together |
| Downward loops, label loops over palettes | loop | yes: write the upward loop, drop the cast |
| Result pins | allocation | sometimes: an early `return x;` |
| Scheduling ties | sched2 | rarely: only where a type or prototype was wrong |
| One variable, two lives | allocation | none: needs a ruling |
| Spill-slot numbering | gcse and reload | none |
| Pool barriers (9) | `arm_reorg` | none |
| Code deleted after allocation | flow | none: the ROM proves removed code, not its shape |

The first five rows are where work still pays. The last four are the
compiler's, and no reshaping of the C will move them.

## Corrections to what was written before

- "An empty C loop is deleted": only when counted in a word and 256 passes or
  fewer.
- A short counter's empty loop is reversed only up to 256 passes, not at any
  count.
- The operand order of a pointer sum is RTL's canonical order, not
  `pointer_int_sum`.
- The dead blocks before or after a call in long routines are more likely
  the first CSE's flush than a scheduling tie.
- The two "priorities one apart" sites needed volatile members, not a change
  to the compiler's timing.

## Questions that are Pascal's

1. The nine pool barriers have no asm-free form. The rules say The Broken
   Seal's barriers are to go; these five cannot, short of a change to the
   compiler (K3).
2. Devices that only steer a numbering (gcse's table, spill slots) cannot be
   argued for or searched for. They could be given a kind of their own, so
   that they stop counting as open work.
3. One variable carrying two values: ruled plain C on 12 November where it
   has no other form. The eight temporaries of this family want the same
   ruling or its refusal.

## What was not checked here

Everything marked Reported. The rounds' reports reached their joins cut
short, and the attempt records hold the rest
(`out/reports/fg/` in the main checkout for the second round). Nothing in
this study changed a compiler, an option or a route.
