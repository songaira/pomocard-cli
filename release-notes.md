## pomocard-cli v0.3.0

Full teardown of the fake tier gate, plus two AI fixes.

### Removed the paywall (full teardown)
- Deleted the Pro/Team gate entirely: no more blurred paywall overlay, no demo `u` plan-cycle, no `upgrade`/`unlock`/`subscribe` commands, no locked views.
- Every view (Board, Analytics, AI Coach, Templates, Team, Billing, Settings) is now open. There are no plans, seats, or payments.
- The only thing standing between you and the AI features is **your own API key** (bring-your-own-key). With no key the app runs fully offline on its built-in local parser.

### Fixed: Google (Gemini) "API key not valid"
- The agent parser was lowercasing the whole command line, so `set key AIzaSy…` got mangled to `aizasy…`. Google keys are case-sensitive, so the API rejected them.
- Keys (and model ids) now preserve their original case. Re-enter your key once after upgrading.

### Fixed: runaway token usage
- Capped LLM output at 1024 tokens across all providers via a single shared constant.
- Google previously had **no** output cap in its `generationConfig`, so Gemini could generate unbounded output and torch your tokens-per-minute quota. Added `maxOutputTokens`.

### Other
- `Billing` view rewritten to an honest "free, local & BYOK" panel (shows your provider/model/key status).
- `Settings` now shows a clean "local account & AI" panel.
- README + tests updated.

### Upgrade note
If you previously set a Google key, re-run `set provider google` then `set key <your exact key>` — the old stored key was the corrupted lowercased copy.
