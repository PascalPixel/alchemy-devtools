// The part of libmgba that `render` uses, as plain functions over one opaque
// machine: load a ROM image held in memory, run one instruction, read and
// write registers and memory, set the keys, and read the picture. A hook on
// the renderer's scanlines changes colours in the picture only, never in the
// palette RAM the game sees: a backdrop colour for each line
// (alchemy_mgba_backdrop) and a recolouring of chosen palette entries
// (alchemy_mgba_recolour).
// Machines are used from one thread at a time.
#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <mgba-util/vfs.h>
#include <mgba/core/core.h>
#include <mgba/core/log.h>
#include <mgba/gba/core.h>
#include <mgba/internal/arm/arm.h>
#include <mgba/internal/arm/isa-inlines.h>
#include <mgba/internal/gba/gba.h>
#include <mgba/internal/gba/video.h>

#define WIDTH 240
#define HEIGHT 160

struct AlchemyStep {
  uint32_t pc;     // the instruction the CPU runs next
  uint32_t frame;  // frames completed since power-on
  int32_t vcount;  // the line being drawn
  int32_t halted;  // whether the CPU sleeps until the next interrupt
};

struct AlchemyMachine {
  struct mCore *core;
  struct GBA *gba;
  struct GBAVideoRenderer *renderer;
  void (*drawScanline)(struct GBAVideoRenderer *, int);
  bool backdropSet;
  uint16_t backdrop[HEIGHT];
  // Palette entries (0 to 511) drawn through the table, BGR555 to BGR555.
  unsigned recolourCount;
  uint16_t recolour[512];
  uint16_t table[0x8000];
  color_t pixels[WIDTH * HEIGHT];
  struct AlchemyMachine *next;
};

// The open machines, so the scanline hook finds its own from the renderer.
static _Thread_local struct AlchemyMachine *gMachines;

static void QuietLog(struct mLogger *logger, int category,
                     enum mLogLevel level, const char *format, va_list args) {
  (void)logger;
  (void)category;
  if (level & mLOG_FATAL) {
    vfprintf(stderr, format, args);
    fputc('\n', stderr);
  }
}

static struct mLogger gLogger = {.log = QuietLog};

// Draws line y with our backdrop colour and recoloured entries in the
// renderer's copy of the palette; the game's palette RAM keeps its own.
static void DrawScanline(struct GBAVideoRenderer *renderer, int y) {
  struct AlchemyMachine *m = gMachines;
  while (m && m->renderer != renderer)
    m = m->next;
  if (!m)
    return;
  if (m->backdropSet && y >= 0 && y < HEIGHT)
    renderer->writePalette(renderer, 0, m->backdrop[y]);
  for (unsigned i = 0; i < m->recolourCount; i++) {
    unsigned entry = m->recolour[i];
    uint16_t colour = m->gba->video.palette[entry] & 0x7fff;
    renderer->writePalette(renderer, entry * 2, m->table[colour]);
  }
  m->drawScanline(renderer, y);
}

struct AlchemyMachine *alchemy_mgba_open(const void *rom, size_t size) {
  struct AlchemyMachine *m = calloc(1, sizeof *m);
  if (!m)
    return NULL;
  mLogSetDefaultLogger(&gLogger);
  m->core = GBACoreCreate();
  if (!m->core || !m->core->init(m->core)) {
    free(m);
    return NULL;
  }
  // Defaults only: no configuration file, BIOS or save is read.
  mCoreInitConfig(m->core, NULL);
  m->core->setVideoBuffer(m->core, m->pixels, WIDTH);
  struct VFile *vf = VFileFromConstMemory(rom, size);
  if (!vf || !m->core->loadROM(m->core, vf)) {
    if (vf)
      vf->close(vf);
    m->core->deinit(m->core);
    free(m);
    return NULL;
  }
  m->core->reset(m->core);
  m->gba = m->core->board;
  m->renderer = m->gba->video.renderer;
  m->drawScanline = m->renderer->drawScanline;
  m->renderer->drawScanline = DrawScanline;
  m->next = gMachines;
  gMachines = m;
  return m;
}

void alchemy_mgba_close(struct AlchemyMachine *m) {
  struct AlchemyMachine **link = &gMachines;
  while (*link && *link != m)
    link = &(*link)->next;
  if (*link)
    *link = m->next;
  m->renderer->drawScanline = m->drawScanline;
  mCoreConfigDeinit(&m->core->config);
  m->core->deinit(m->core);
  free(m);
}

void alchemy_mgba_step(struct AlchemyMachine *m, struct AlchemyStep *out) {
  struct ARMCore *cpu = m->core->cpu;
  m->core->step(m->core);
  out->pc = cpu->gprs[ARM_PC] -
            (cpu->cpsr.t ? WORD_SIZE_THUMB : WORD_SIZE_ARM);
  out->frame = m->core->frameCounter(m->core);
  out->vcount = m->gba->video.vcount;
  out->halted = cpu->halted;
}

uint32_t alchemy_mgba_register(struct AlchemyMachine *m, unsigned n) {
  struct ARMCore *cpu = m->core->cpu;
  return n < 16 ? (uint32_t)cpu->gprs[n] : 0;
}

void alchemy_mgba_set_register(struct AlchemyMachine *m, unsigned n,
                               uint32_t value) {
  struct ARMCore *cpu = m->core->cpu;
  if (n < ARM_PC)
    cpu->gprs[n] = (int32_t)value;
}

// Continues at a Thumb address; the CPU is in Thumb state.
void alchemy_mgba_branch(struct AlchemyMachine *m, uint32_t address) {
  struct ARMCore *cpu = m->core->cpu;
  cpu->gprs[ARM_PC] = (int32_t)(address & ~1u);
  ThumbWritePC(cpu);
}

uint32_t alchemy_mgba_read(struct AlchemyMachine *m, uint32_t address,
                           unsigned size) {
  switch (size) {
  case 1:
    return m->core->rawRead8(m->core, address, -1);
  case 2:
    return m->core->rawRead16(m->core, address, -1);
  default:
    return m->core->rawRead32(m->core, address, -1);
  }
}

void alchemy_mgba_write(struct AlchemyMachine *m, uint32_t address,
                        unsigned size, uint32_t value) {
  switch (size) {
  case 1:
    m->core->rawWrite8(m->core, address, -1, (uint8_t)value);
    break;
  case 2:
    m->core->rawWrite16(m->core, address, -1, (uint16_t)value);
    break;
  default:
    m->core->rawWrite32(m->core, address, -1, value);
    break;
  }
}

void alchemy_mgba_set_keys(struct AlchemyMachine *m, uint32_t keys) {
  m->core->setKeys(m->core, keys);
}

// The last picture drawn: 240 by 160 pixels, red in the low byte.
const color_t *alchemy_mgba_pixels(const struct AlchemyMachine *m) {
  return m->pixels;
}

// A BGR555 backdrop colour for each line from now on, or null for the
// game's own.
void alchemy_mgba_backdrop(struct AlchemyMachine *m, const uint16_t *lines) {
  m->backdropSet = lines != NULL;
  if (lines)
    memcpy(m->backdrop, lines, sizeof m->backdrop);
}

// From now on draws each of `count` palette entries (0 to 255 BG, 256 to 511
// OBJ) as table[its colour in palette RAM]; a count of 0 stops.
void alchemy_mgba_recolour(struct AlchemyMachine *m, const uint16_t *entries,
                           unsigned count, const uint16_t *table) {
  if (count > 512)
    count = 512;
  m->recolourCount = count;
  if (count) {
    memcpy(m->recolour, entries, count * sizeof *entries);
    memcpy(m->table, table, sizeof m->table);
  }
}

// The size of a saved machine state, and saving and loading one.
size_t alchemy_mgba_state_size(struct AlchemyMachine *m) {
  return m->core->stateSize(m->core);
}

bool alchemy_mgba_save_state(struct AlchemyMachine *m, void *state) {
  return m->core->saveState(m->core, state);
}

bool alchemy_mgba_load_state(struct AlchemyMachine *m, const void *state) {
  return m->core->loadState(m->core, state);
}

// Shows or hides a layer in the picture: 0 to 3 the backgrounds, 4 the
// sprites. The game's registers are unchanged.
void alchemy_mgba_layer(struct AlchemyMachine *m, unsigned layer, bool shown) {
  m->core->enableVideoLayer(m->core, layer, shown);
}

// Stores a halfword as the processor would, so an I/O register's write
// takes effect (rawWrite16 only changes the memory behind it).
void alchemy_mgba_bus_write16(struct AlchemyMachine *m, uint32_t address,
                              uint16_t value) {
  m->core->busWrite16(m->core, address, value);
}
