; hello-gb -- the smallest possible gbdev-forge example.
;
; Shows the intended asset-pipeline -> game-code path: `smiley.png` (a
; hand-drawn 16x16, 4-shade DMG image, two 8x8 tiles side by side) was run
; through `forge-png2tile` to produce `smiley_tiles.asm` (see
; ../README.md for the exact command). This file is a minimal RGBDS program
; that copies those tiles into VRAM and displays them as the top-left two
; background tiles.
;
; NOT YET ASSEMBLED/RUN: this sandbox has no RGBDS (`rgbasm`/`rgblink`/
; `rgbfix`) installed -- see ../README.md for exactly what was and wasn't
; verified. Written to the real RGBDS syntax/hardware register layout, but
; treat it as unverified source, not a working ROM, until it has actually
; been run through RGBDS and booted.

; Hardware registers used below (normally pulled in via RGBDS's own
; hardware.inc; spelled out here since this sandbox has no RGBDS install to
; fetch it from, and the point of this file is to be self-contained).
DEF rLCDC   EQU $FF40
DEF rLY     EQU $FF44
DEF rBGP    EQU $FF47

DEF LCDCF_ON      EQU %10000000
DEF LCDCF_BG8000  EQU %00010000 ; BG/window tile data at $8000-$8FFF, unsigned indexing
DEF LCDCF_BGON    EQU %00000001

SECTION "Tiles", ROM0
; smiley_tiles: 2 unique 8x8 tiles (16 bytes each = 32 bytes total), from
; `forge-png2tile`'s --out-asm output -- see ../README.md for the command.
INCLUDE "smiley_tiles.asm"

SECTION "Header", ROM0[$100]
    jp Start
    ds $150 - @, 0 ; RGBDS's rgbfix normally patches the rest of the header

SECTION "Start", ROM0[$150]
Start:
    ; Wait for VBlank before touching VRAM, since the PPU owns it otherwise.
.waitVBlank:
    ld a, [rLY]
    cp 144
    jp c, .waitVBlank

    ; Turn the LCD off so VRAM writes below are always safe regardless of
    ; PPU mode (simpler and more common in minimal examples than a second
    ; VBlank-gated copy loop).
    xor a
    ld [rLCDC], a

    ; Copy smiley_tiles (2 unique 8x8 tiles, 16 bytes each -- see
    ; smiley.map.json for which source tile maps to which, and flip bits)
    ; to the start of tile data block 0 ($8000).
    ld hl, $8000
    ld de, smiley_tiles
    ld bc, 32 ; 2 unique tiles x 16 bytes/tile, per smiley.2bpp's file size
.copyTiles:
    ld a, [de]
    ld [hli], a
    inc de
    dec bc
    ld a, b
    or c
    jp nz, .copyTiles

    ; Write the background tile map's first two entries: tile 0 then tile 1
    ; ($9800, $9801). The rest of the 32x32 tilemap is left at its post-boot
    ; value (all zero, i.e. all tile 0), so the visible 20x18 screen actually
    ; tiles with repeated smiley-tile 0/1 pairs rather than showing a single
    ; smiley in isolation -- staying minimal rather than adding a tilemap-
    ; clearing loop just to blank the rest to some third, unused tile.
    ld hl, $9800
    ld a, 0
    ld [hli], a
    ld a, 1
    ld [hl], a

    ; Standard DMG palette: 00=white .. 11=black, brightest index first.
    ld a, %11100100
    ld [rBGP], a

    ; LCD on, background on.
    ld a, LCDCF_ON | LCDCF_BG8000 | LCDCF_BGON
    ld [rLCDC], a

.hang:
    jp .hang
