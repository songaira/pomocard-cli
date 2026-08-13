## pomocard-cli v0.4.0

Customizable agent persona.

### New: agent persona
- `set agent persona warm | cold | stoic | chaotic | balanced` — flavors the AI Coach
  voice and the agent summary sign-off (default `balanced`).
- `set agent prompt <text>` — full system-prompt hotswap: `<text>` becomes the agent's
  own system prompt, replacing the built-in coach prompt entirely.
- `set agent reset` — back to `balanced`.
- Persona is stored locally and only affects tone. It never changes how commands are
  parsed, so the command translator stays strictly formatted.

### Notes
Builds on v0.3.0 (removed the tier gate, gated AI on your API key, fixed Google key
case-sensitivity, capped LLM output tokens to 1024, edition 2024).
