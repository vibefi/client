export const sharedStyles = `
  :root { color-scheme: light; }
  * { box-sizing: border-box; }
  body {
    font-family: system-ui, -apple-system, BlinkMacSystemFont, sans-serif;
    margin: 0;
    background: #f8fafc;
    color: #0f172a;
  }
  button {
    padding: 10px 14px;
    border-radius: 10px;
    border: 1px solid #cbd5e1;
    background: #fff;
    cursor: pointer;
    font-size: 13px;
  }
  button:hover { background: #f1f5f9; }
  button:disabled {
    opacity: 0.6;
    cursor: default;
  }
  button.primary {
    background: #0f172a;
    color: #fff;
    border-color: #0f172a;
  }
  button.primary:hover { background: #1e293b; }
  button.secondary {
    border-color: #cbd5e1;
    background: #fff;
  }
  button.secondary:hover { background: #f1f5f9; }
  .subtitle {
    color: #475569;
    margin-bottom: 24px;
    font-size: 14px;
  }
`;

export const sharedPageStyles = `
  .page-container {
    margin: 40px auto;
    padding: 32px;
  }
  .page-container.compact {
    max-width: 480px;
    margin-top: 60px;
  }
  .page-container.wide { max-width: 620px; }
  .page-title {
    font-size: 22px;
    margin: 0 0 6px;
  }
`;

export const sharedFormFieldStyles = `
  .field label {
    display: block;
    font-size: 12px;
    color: #64748b;
    margin-bottom: 4px;
  }
  .field input {
    width: 100%;
    padding: 8px 10px;
    border: 1px solid #e2e8f0;
    border-radius: 8px;
    font-size: 13px;
    background: #fff;
  }
  .field input:focus { outline: none; border-color: #94a3b8; }
  .field input:disabled { background: #f8fafc; color: #94a3b8; cursor: default; }
`;

export const sharedFeedbackStyles = `
  .status { font-size: 13px; margin-top: 8px; }
  .status.ok { color: #0f766e; }
  .status.err { color: #dc2626; }
  .error {
    color: #dc2626;
    font-size: 13px;
    margin-top: 8px;
  }
  .empty {
    color: #94a3b8;
    font-size: 13px;
    padding: 12px 0;
  }
`;

export const sharedSurfaceStyles = `
  .surface-card {
    border: 1px solid #e2e8f0;
    border-radius: 10px;
    background: #fff;
  }
`;

export const sharedUtilityStyles = `
  .flex-1 { flex: 1; }
  .flex-2 { flex: 2; }
  .mt-0 { margin-top: 0 !important; }
  .mb-0 { margin-bottom: 0 !important; }
  .mb-12 { margin-bottom: 12px !important; }
`;

export function composeStyles(...styleBlocks: string[]): string {
  return styleBlocks.filter(Boolean).join("\n");
}
