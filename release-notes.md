## pomocard-cli v0.4.1

Bug fix for agent persona.

### Fixed
- `set agent persona <x>` now clears any previously-set custom prompt, so the
  preset actually takes effect. Before this fix a lingering `set agent prompt <text>`
  would silently override the persona (e.g. `warm` kept replying as the old custom
  voice). Persona presets and custom prompts are now mutually exclusive.

### Notes
Builds on v0.4.0 (customizable agent persona: `set agent persona warm|cold|stoic|
chaotic|balanced`, `set agent prompt <text>` hotswap, `set agent reset`).
