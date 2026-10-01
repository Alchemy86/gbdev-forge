# gbdev-forge

A code-first Rust toolchain for building classic Game Boy games now, with Game Boy Advance
support planned later. No drag-and-drop, no node-based visual authoring - real code, a real
built-in run/debug layer, and a shared virtual-hardware model used by both static checks and
live runtime instrumentation.

Design spec: see the firstmate fleet's `data/gbdev-ide-spec/report.md`.

Status: early build. First increment in progress: `forge-png2tile`, a PNG -> Game Boy 2BPP
tile converter with flip-aware smart tile deduplication.
