import {
  composeStyles,
  sharedFeedbackStyles,
  sharedFormFieldStyles,
  sharedPageStyles,
  sharedStyles,
  sharedSurfaceStyles,
} from "../styles/shared";

export const localStyles = `
  html, body, #root {
    height: 100%;
    overflow: hidden;
    margin: 0;
  }
  .page-container.code-page {
    --ide-bg: #060d18;
    --ide-surface: #0d1627;
    --ide-surface-2: #111f35;
    --ide-border: #22324c;
    --ide-border-strong: #2f4668;
    --ide-text: #d7e2f2;
    --ide-text-dim: #90a4c2;
    --ide-accent: #4fd1c5;
    --ide-accent-soft: rgba(79, 209, 197, 0.16);
    --ide-warning: #facc15;
    --ide-danger: #fb7185;
    max-width: none;
    height: 100vh;
    overflow: hidden;
    background: radial-gradient(circle at 16% -4%, #1d3354 0%, transparent 45%),
      radial-gradient(circle at 90% 0%, #1b2d4a 0%, transparent 40%), var(--ide-bg);
    color: var(--ide-text);
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
  }
  .code-page .page-title {
    margin: 0;
    font-size: 16px;
    letter-spacing: 0.02em;
    color: #f4f7ff;
  }
  .code-page .subtitle {
    margin-top: 2px;
    color: var(--ide-text-dim);
    font-size: 11px;
  }
  .code-page .surface-card {
    background: linear-gradient(165deg, rgba(17, 31, 53, 0.94), rgba(13, 22, 39, 0.96));
    border: 1px solid var(--ide-border);
    box-shadow: 0 16px 36px rgba(1, 6, 17, 0.4);
  }
  .code-page input,
  .code-page select,
  .code-page textarea {
    border: 1px solid var(--ide-border-strong);
    border-radius: 8px;
    background: #0b1323;
    color: #e6edf8;
  }
  .code-page input::placeholder,
  .code-page textarea::placeholder {
    color: #7184a1;
  }
  .code-page .field label {
    color: var(--ide-text-dim);
    font-size: 11px;
    letter-spacing: 0.02em;
  }
  .code-page button.primary {
    background: linear-gradient(140deg, #2dd4bf, #14b8a6);
    color: #052622;
    border: 1px solid #58ddd0;
    font-weight: 600;
  }
  .code-page button.secondary {
    border: 1px solid var(--ide-border-strong);
    background: #0f1b31;
    color: #d2deef;
  }
  .code-page button.secondary:hover:not(:disabled) {
    border-color: #486589;
    background: #162743;
  }
  .code-page button:disabled {
    opacity: 0.55;
  }
  .actions {
    margin-top: 10px;
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }
  .section-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 10px;
    margin-bottom: 10px;
  }
  .section-head h2,
  .section-head h3 {
    margin: 0;
    font-size: 13px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--ide-text-dim);
    font-weight: 600;
  }
  .ide-shell {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
    padding: 8px;
    gap: 8px;
  }
  .ide-topbar {
    flex-shrink: 0;
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
    padding: 8px 12px;
    border-radius: 10px;
  }
  .ide-topbar-main {
    min-width: 0;
  }
  .ide-topbar-actions {
    display: flex;
    gap: 8px;
    align-items: center;
    flex-wrap: wrap;
    justify-content: flex-end;
  }
  .mode-toggle {
    display: inline-flex;
    gap: 4px;
    background: #0a1427;
    border: 1px solid var(--ide-border);
    border-radius: 10px;
    padding: 4px;
  }
  .mode-toggle button {
    border: 1px solid transparent;
    border-radius: 8px;
    background: transparent;
    color: var(--ide-text-dim);
    height: 28px;
    padding: 0 10px;
    font-size: 12px;
  }
  .mode-toggle button.active {
    background: var(--ide-accent-soft);
    color: #bffaf3;
    border: 1px solid rgba(79, 209, 197, 0.45);
  }
  .ide-workspace {
    display: flex;
    flex-direction: row;
    flex: 1;
    min-height: 0;
    overflow: hidden;
    gap: 8px;
  }
  .ide-sidebar {
    flex-shrink: 0;
    width: 260px;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px;
  }
  .sidebar-tabs {
    display: flex;
    gap: 4px;
    background: #0a1427;
    border: 1px solid var(--ide-border);
    border-radius: 10px;
    padding: 4px;
    overflow-x: auto;
    flex-shrink: 0;
  }
  .sidebar-tab {
    border: 1px solid transparent;
    background: transparent;
    color: var(--ide-text-dim);
    border-radius: 8px;
    height: 30px;
    font-size: 11px;
    flex: 1 1 0;
    min-width: 0;
    white-space: nowrap;
    padding: 0 10px;
  }
  .sidebar-tab.active {
    background: var(--ide-accent-soft);
    border: 1px solid rgba(79, 209, 197, 0.55);
    color: #c4fcf6;
  }
  .sidebar-panel {
    border: 1px solid var(--ide-border);
    border-radius: 10px;
    background: rgba(8, 14, 27, 0.68);
    padding: 10px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-height: 0;
    overflow: hidden;
    flex: 1;
  }
  .sidebar-scroll {
    min-height: 0;
    overflow: auto;
    padding-right: 2px;
    flex: 1;
  }
  .panel-block + .panel-block {
    border-top: 1px solid rgba(47, 70, 104, 0.55);
    padding-top: 10px;
    margin-top: 6px;
  }
  .panel-block h3 {
    margin: 0 0 8px;
    font-size: 12px;
    color: #d8e8ff;
  }
  .project-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .project-item {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 10px;
    padding: 10px;
    border: 1px solid rgba(47, 70, 104, 0.5);
    border-radius: 9px;
    background: rgba(12, 22, 40, 0.9);
  }
  .project-name {
    font-size: 13px;
    font-weight: 600;
    color: #ecf3ff;
  }
  .project-path {
    margin-top: 3px;
    font-family: ui-monospace, Menlo, Monaco, Consolas, monospace;
    font-size: 11px;
    color: #9eb4d4;
    word-break: break-all;
  }
  .project-meta {
    margin-top: 3px;
    font-size: 11px;
    color: #7e94b4;
  }
  .dev-server-status {
    font-size: 12px;
    color: #cddaf0;
  }
  .dev-server-status code {
    font-family: ui-monospace, Menlo, Monaco, Consolas, monospace;
  }
  .file-open-row {
    display: flex;
    gap: 8px;
    margin-bottom: 10px;
    flex-shrink: 0;
  }
  .file-open-row input {
    flex: 1;
    min-width: 0;
  }
  .tree-wrap {
    border: 1px solid var(--ide-border);
    border-radius: 9px;
    background: #090f1d;
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 6px;
  }
  .tree-item {
    display: block;
    width: 100%;
    text-align: left;
    border: 0;
    background: transparent;
    color: #d0ddf2;
    font-size: 12px;
    padding: 6px;
    border-radius: 6px;
    cursor: pointer;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tree-item:hover {
    background: #152947;
  }
  .tree-item.active {
    background: #1d365b;
    color: #f2f7ff;
    font-weight: 600;
  }
  .tree-empty {
    color: #7f95b6;
    font-size: 12px;
    padding: 8px;
  }
  .ide-main {
    flex: 1;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    display: grid;
    gap: 8px;
  }
  .ide-main.mode-llm-code-preview {
    grid-template-columns: minmax(0, 1.2fr) minmax(280px, 0.8fr);
  }
  .ide-main.mode-llm-preview {
    grid-template-columns: minmax(0, 1fr);
  }
  .editor-shell {
    border: 1px solid var(--ide-border);
    border-radius: 10px;
    background: #090f1d;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    height: 100%;
    min-width: 0;
  }
  .editor-tabs {
    display: flex;
    gap: 2px;
    align-items: stretch;
    border-bottom: 1px solid #16253d;
    background: #060c18;
    overflow-x: auto;
    padding: 4px;
    flex-shrink: 0;
  }
  .editor-tab {
    border: 1px solid transparent;
    background: #0f1d33;
    color: #aabdd8;
    border-radius: 8px;
    height: 32px;
    padding: 0 10px;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    cursor: pointer;
    white-space: nowrap;
    font-size: 12px;
  }
  .editor-tab.active {
    border-color: #2a4368;
    background: #1a2f51;
    color: #eff4ff;
  }
  .editor-tab-close {
    border: 0;
    background: transparent;
    color: #90a5c3;
    cursor: pointer;
    font-size: 12px;
    padding: 0;
    line-height: 1;
  }
  .editor-tab-close:hover {
    color: #f2f7ff;
  }
  .editor-dirty {
    color: var(--ide-warning);
    font-size: 12px;
  }
  .editor-toolbar {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 8px;
    padding: 8px 10px;
    border-bottom: 1px solid #16253d;
    background: #0b1527;
    flex-shrink: 0;
  }
  .editor-path {
    color: #dce7f8;
    font-size: 12px;
    font-family: ui-monospace, Menlo, Monaco, Consolas, monospace;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .editor-status {
    color: #8fa4c3;
    font-size: 11px;
  }
  .editor-codemirror {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }
  .editor-codemirror .cm-editor {
    height: 100%;
    background: #090f1d;
  }
  .editor-codemirror .cm-scroller {
    font-family: ui-monospace, Menlo, Monaco, Consolas, monospace;
    font-size: 12px;
    line-height: 1.55;
  }
  .editor-codemirror .cm-content {
    padding: 12px;
  }
  .editor-codemirror .cm-gutters {
    background: #090f1d;
    border-right: 1px solid #16253d;
    color: #5e7392;
  }
  .editor-placeholder {
    color: #8fa4c3;
    padding: 16px;
    font-size: 12px;
    flex: 1;
  }
  .preview-panel {
    border: 1px solid var(--ide-border);
    border-radius: 10px;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    min-width: 0;
    background: #090f1d;
  }
  .preview-toolbar {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 8px;
    padding: 8px 10px;
    border-bottom: 1px solid #16253d;
    background: #0b1527;
    font-size: 12px;
    color: #b5c9e6;
    flex-shrink: 0;
  }
  .preview-frame-wrap {
    flex: 1;
    min-height: 0;
    overflow: hidden;
    background: #060c17;
  }
  .preview-frame {
    display: block;
    width: 100%;
    height: 100%;
    border: 0;
    background: #ffffff;
  }
  .preview-fallback {
    flex: 1;
    border: 1px dashed #2a4160;
    border-radius: 10px;
    margin: 14px;
    background: #0d192d;
    color: #8ca2c0;
    font-size: 13px;
    padding: 14px;
    display: flex;
    align-items: center;
    justify-content: center;
    text-align: center;
  }
  .console-panel-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
  }
  .console-pre {
    margin: 0;
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 12px;
    font-size: 12px;
    color: #dce7f8;
    font-family: ui-monospace, Menlo, Monaco, Consolas, monospace;
    white-space: pre-wrap;
    word-break: break-word;
    background: #090f1d;
  }
  .sidebar-console {
    border: 1px solid var(--ide-border);
    border-radius: 8px;
    flex: 1;
    min-height: 0;
  }
  .console-line {
    white-space: pre-wrap;
    word-break: break-word;
  }
  .console-link {
    border: 0;
    background: transparent;
    color: #6bd9f4;
    cursor: pointer;
    font: inherit;
    padding: 0;
    text-decoration: underline;
  }
  .console-link:hover {
    color: #baf0ff;
  }
  .diff-pre {
    margin: 0;
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 12px;
    font-size: 12px;
    font-family: ui-monospace, Menlo, Monaco, Consolas, monospace;
    white-space: pre;
    color: #e2e8f0;
    background: #0f172a;
  }
  .diff-line.file {
    color: #f8fafc;
    font-weight: 600;
  }
  .diff-line.meta {
    color: #93c5fd;
  }
  .diff-line.hunk {
    color: #c4b5fd;
  }
  .diff-line.add {
    color: #86efac;
    background: rgba(22, 163, 74, 0.12);
  }
  .diff-line.remove {
    color: #fca5a5;
    background: rgba(220, 38, 38, 0.12);
  }
  .diff-line.base {
    color: #cbd5e1;
  }
  .chat-gear-btn {
    border: 0;
    background: transparent;
    color: var(--ide-text-dim);
    cursor: pointer;
    font-size: 14px;
    padding: 2px 4px;
    border-radius: 4px;
    line-height: 1;
  }
  .chat-gear-btn:hover {
    color: var(--ide-text);
    background: rgba(255,255,255,0.07);
  }
  .chat-gear-btn.active {
    color: var(--ide-accent);
  }
  .chat-settings-panel {
    border-bottom: 1px solid var(--ide-border);
    padding: 8px 10px;
    flex-shrink: 0;
    background: rgba(6, 12, 24, 0.5);
  }
  .chat-shell {
    flex: 1;
    min-height: 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px;
  }
  .chat-history {
    flex: 1;
    min-height: 0;
    overflow: auto;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .chat-placeholder {
    color: #7f95b6;
    font-size: 12px;
  }
  .chat-message {
    padding: 8px 10px;
    border-radius: 8px;
    max-width: 95%;
    white-space: pre-wrap;
    word-break: break-word;
    font-size: 12px;
    line-height: 1.45;
  }
  .chat-message.user {
    align-self: flex-end;
    background: #143562;
    color: #d4e8ff;
    border: 1px solid rgba(96, 165, 250, 0.35);
  }
  .chat-message.assistant {
    align-self: flex-start;
    background: #101f36;
    color: #dce8fb;
    border: 1px solid rgba(59, 130, 246, 0.25);
  }
  .chat-message.assistant p {
    margin: 0 0 8px;
  }
  .chat-message.assistant p:last-child {
    margin-bottom: 0;
  }
  .chat-message.assistant ul,
  .chat-message.assistant ol {
    margin: 0 0 8px 18px;
    padding: 0;
  }
  .chat-message.assistant code {
    font-family: ui-monospace, Menlo, Monaco, Consolas, monospace;
    font-size: 11px;
    background: #213759;
    border-radius: 4px;
    padding: 1px 4px;
  }
  .chat-message.assistant pre {
    margin: 8px 0 0;
    background: #090f1d;
    color: #dce8fb;
    border-radius: 8px;
    padding: 8px;
    overflow: auto;
    border: 1px solid rgba(66, 90, 126, 0.4);
  }
  .chat-message.assistant pre code {
    background: transparent;
    color: inherit;
    padding: 0;
    font-size: 11px;
  }
  .tool-calls {
    margin-top: 8px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .tool-call-card {
    border: 1px solid #2c4061;
    border-radius: 8px;
    background: #0b162a;
    padding: 6px 8px;
    font-size: 11px;
    line-height: 1.4;
  }
  .tool-call-card.ok {
    border-color: #1f7a56;
    background: #092016;
  }
  .tool-call-card.err {
    border-color: #8a2c4a;
    background: #280d17;
  }
  .tool-call-toggle {
    width: 100%;
    border: 0;
    background: transparent;
    display: flex;
    align-items: center;
    gap: 8px;
    text-align: left;
    color: inherit;
    cursor: pointer;
    font: inherit;
    padding: 0;
  }
  .tool-call-output {
    margin-top: 4px;
    color: #9eb4d4;
  }
  .tool-call-content {
    margin: 6px 0 0;
    border: 1px solid #2c4061;
    border-radius: 6px;
    background: #071022;
    color: #d7e2f2;
    font-size: 11px;
    max-height: 140px;
    overflow: auto;
    padding: 6px;
    white-space: pre-wrap;
    word-break: break-word;
  }
  .chat-change-summary {
    margin-top: 8px;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 4px 8px;
    border-radius: 999px;
    border: 1px solid #395577;
    background: #0e1a2f;
    color: #dce7f8;
    font-size: 11px;
  }
  .chat-change-summary button {
    font-size: 11px;
    padding: 2px 7px;
  }
  .chat-meta-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
  }
  .chat-meta {
    font-size: 11px;
    color: #90a4c2;
  }
  .chat-stream-status {
    flex-shrink: 0;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    align-self: flex-start;
    max-width: 100%;
    border: 1px solid #2f4668;
    border-radius: 999px;
    background: #0e1a2f;
    color: #cfe0fb;
    font-size: 11px;
    padding: 5px 10px;
  }
  .chat-stream-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--ide-accent);
    box-shadow: 0 0 0 0 rgba(79, 209, 197, 0.45);
    animation: chat-stream-pulse 1.2s ease-in-out infinite;
  }
  @keyframes chat-stream-pulse {
    0% {
      transform: scale(1);
      box-shadow: 0 0 0 0 rgba(79, 209, 197, 0.45);
    }
    70% {
      transform: scale(1.1);
      box-shadow: 0 0 0 5px rgba(79, 209, 197, 0);
    }
    100% {
      transform: scale(1);
      box-shadow: 0 0 0 0 rgba(79, 209, 197, 0);
    }
  }
  .chat-input-row {
    display: flex;
    gap: 8px;
    align-items: flex-end;
    flex-shrink: 0;
  }
  .chat-input-row textarea {
    flex: 1;
    min-height: 54px;
    resize: vertical;
  }
  .chat-settings-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 8px;
  }
  .chat-settings-grid .field {
    margin: 0;
  }
  .chat-settings-grid label {
    display: block;
    margin-bottom: 4px;
    font-size: 11px;
    color: #90a4c2;
  }
  .chat-settings-grid input,
  .chat-settings-grid select {
    width: 100%;
  }
  .quick-open-overlay {
    position: fixed;
    inset: 0;
    background: rgba(2, 6, 23, 0.55);
    display: flex;
    align-items: flex-start;
    justify-content: center;
    padding: 8vh 12px 12px;
    z-index: 1000;
  }
  .quick-open-modal {
    width: min(760px, 100%);
    border: 1px solid #334155;
    border-radius: 12px;
    background: #0f172a;
    box-shadow: 0 18px 40px rgba(15, 23, 42, 0.45);
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .quick-open-modal input {
    width: 100%;
    border: 1px solid #334155;
    border-radius: 8px;
    background: #111827;
    color: #e2e8f0;
    font-family: ui-monospace, Menlo, Monaco, Consolas, monospace;
  }
  .quick-open-results {
    border: 1px solid #334155;
    border-radius: 8px;
    background: #111827;
    max-height: 340px;
    overflow: auto;
  }
  .quick-open-result {
    width: 100%;
    border: 0;
    background: transparent;
    color: #cbd5e1;
    text-align: left;
    padding: 8px 10px;
    cursor: pointer;
    font-family: ui-monospace, Menlo, Monaco, Consolas, monospace;
    font-size: 12px;
  }
  .quick-open-result:hover,
  .quick-open-result.active {
    background: #1e293b;
  }
  .quick-open-empty {
    padding: 10px;
    color: #94a3b8;
    font-size: 12px;
  }
  .command-palette-empty {
    padding: 18px 10px;
    color: #94a3b8;
    font-size: 12px;
    text-align: center;
  }
  @media (max-width: 980px) {
    .ide-workspace {
      flex-direction: column;
    }
    .ide-sidebar {
      width: 100%;
      flex-shrink: 0;
      max-height: 200px;
    }
    .ide-main.mode-llm-code-preview {
      grid-template-columns: minmax(0, 1fr);
    }
  }
  @media (max-width: 760px) {
    .ide-topbar {
      flex-direction: column;
      align-items: stretch;
    }
    .ide-topbar-actions {
      justify-content: flex-start;
    }
    .sidebar-tabs {
      overflow-x: auto;
    }
    .file-open-row {
      flex-direction: column;
    }
    .chat-input-row {
      flex-direction: column;
      align-items: stretch;
    }
    .chat-settings-grid {
      grid-template-columns: 1fr;
    }
  }
`;

export const styles = composeStyles(
  sharedStyles,
  sharedPageStyles,
  sharedFormFieldStyles,
  sharedFeedbackStyles,
  sharedSurfaceStyles,
  localStyles
);
