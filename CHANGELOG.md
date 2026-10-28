# Changelog

## 0.2.0 — 2026-10-29

### Added

- Charts and Compact Overview modes. Compact combines resource micro-meters with
  a Top Processes strip showing CPU and resident memory per app.
- Mode switching preserves the selected history range and paused inspection.

### Fixed

- Process-memory rankings compare byte totals exactly, including values above
  JavaScript's safe integer range, and reject overflowing totals.
- The Compact memory-list total includes all returned memory entries, with the
  five-column display limit and unavailable readings identified explicitly.
- Interrupted updates retain labeled last-valid readings and timestamps while
  clearing activity bars, including at the smaller supported test viewport.
- Preference-reset, GPU-fallback and settings-save failure notices remain visible
  across collected samples until a successful explicit save or session close.

### Validation limits

- Installers are unsigned; signing and automatic updates are not configured.
- Native Windows/Ubuntu hardware acceptance, installed-package/offline checks,
  suspend/resume and multi-GPU scenarios remain pending.
- Reference CPU/memory measurements and the full 24-hour soak remain pending.
- Automated unit, integration and browser checks do not replace these native
  acceptance and performance results.
