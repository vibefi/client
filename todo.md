# VibeFi Code TODO (current backlog)

## Status

Core v1 vibe-coding flow is implemented (spec stages 1-8 at a functional level). Remaining work is mostly polish, missing UX features, testing, and production hardening.

## P1: Complete the current UX/spec gaps

- [x] Add file type icons in the file tree (spec §9). (Implemented with `react-icons` + compact extension badges.)
- [x] Add recursive directory delete support (`code_deleteDir` IPC + UI action + confirmation).
- [x] Add resizable pane dividers (sidebar width, preview width, chat height/collapse) (spec §2). (Chat resize/collapse implemented for the composer pane within the Chat tab.)
- [x] Persist last-opened project and restore it on next Code-tab open (spec §15 / Stage 8 polish).
- [x] Implement the full welcome/onboarding flow when no API key is configured (spec §15).

## P1/P2: Code quality and maintenance

- [ ] Decide whether to wire `internal-ui/src/code/chat/llm/aiSdkTools.ts` into `provider.ts` or remove it to avoid dead-path confusion.
- [ ] Add automated tests for:
- [ ] SSE parsing (`claude.ts`, `openai.ts`, `sse.ts`)
- [ ] tool-calling loop / stream glue (`provider.ts`, `tools.ts`)
- [ ] diff formatting/rendering helpers (`editor/diff.ts`)


## P3: Nice-to-have polish (from spec / expected IDE behavior)

- [ ] Resizable/collapsible chat pane polish beyond minimum split sizing (smooth UX + persistence).
- [ ] Improve responsive behavior for narrow windows (collapse/stack panels more cleanly).
- [ ] Expand error surfacing/toasts for IPC and provider failures.
- [ ] Optional tab drag-reorder and more editor ergonomics.

## Future Enhancements (not required for current spike)

- [ ] Ollama / local LLM provider
- [ ] Publish-to-IPFS flow from Code tab
- [ ] LSP integration (TS autocomplete, diagnostics, go-to-definition)
