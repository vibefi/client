import { useEffect, useMemo, useRef, useState } from "react";
import type { IpcClient } from "../../ipc/client";
import { PROVIDER_IDS } from "../../ipc/contracts";
import { buildUnifiedDiffForChanges, type DiffChange } from "../editor/diff";
import type { ChatTab, DiffTab, EditorTab, FileTab } from "../types";
import {
  asErrorMessage,
  createChatTab,
  createConsoleTab,
  createDiffTab,
  isChatTab,
  isFileTab,
  isFileTabDirty,
  isDiffTab,
  normalizeConsolePathForProject,
  parseReadFileResult,
  tabIdForPath,
} from "../utils";
import { AUTO_SAVE_DEBOUNCE_MS, CHAT_TAB_ID, CONSOLE_TAB_ID, DIFF_TAB_ID } from "../constants";
import type { ConsoleHook } from "./useConsole";

export interface EditorHook {
  openTabs: EditorTab[];
  openTabsRef: React.MutableRefObject<EditorTab[]>;
  activeTabId: string;
  activeTabIdRef: React.MutableRefObject<string>;
  activeTab: EditorTab | null;
  activeFileTab: FileTab | null;
  activeDiffTab: DiffTab | null;
  activeChatTab: ChatTab | null;
  pendingLineJump: { tabId: string; line: number; nonce: number } | null;
  lastChangeSet: DiffChange[];
  setLastChangeSet: (changes: DiffChange[]) => void;
  setPendingLineJump: React.Dispatch<
    React.SetStateAction<{ tabId: string; line: number; nonce: number } | null>
  >;
  openFileTab: (
    filePath: string,
    options?: {
      projectPath?: string;
      showStatus?: boolean;
      silentError?: boolean;
      activate?: boolean;
    }
  ) => Promise<boolean>;
  closeTab: (tabId: string) => Promise<void>;
  activateTab: (tabId: string) => Promise<void>;
  saveActiveTab: (options?: { announce?: boolean }) => Promise<{ error?: string; status?: string }>;
  handleActiveEditorChange: (content: string) => void;
  scheduleAutoSave: (tabId: string) => void;
  clearAutoSaveTimer: () => void;
  replaceOpenFileTabContent: (filePath: string, content: string) => void;
  closeOpenFileTab: (filePath: string) => void;
  openOrUpdateDiffTab: (diffText: string, options?: { activate?: boolean }) => void;
  openLatestDiff: () => void;
  handleCodeFileChanged: (value: unknown) => Promise<void>;
  openFileAtLocation: (filePath: string, line: number) => Promise<{ error?: string; status?: string }>;
  applyOpenedProject: () => void;
  readFileSnapshot: (projectPath: string, filePath: string) => Promise<string | null>;
}

export function useEditor(
  client: IpcClient,
  activeProjectPath: string,
  console_: ConsoleHook
): EditorHook {
  const [openTabs, setOpenTabs] = useState<EditorTab[]>([createConsoleTab(), createChatTab()]);
  const [activeTabId, setActiveTabId] = useState<string>(CONSOLE_TAB_ID);
  const [pendingLineJump, setPendingLineJump] = useState<{
    tabId: string;
    line: number;
    nonce: number;
  } | null>(null);
  const [lastChangeSet, setLastChangeSet] = useState<DiffChange[]>([]);

  const openTabsRef = useRef<EditorTab[]>(openTabs);
  const activeTabIdRef = useRef(activeTabId);
  const activeProjectPathRef = useRef(activeProjectPath);
  const autoSaveTimerRef = useRef<number | null>(null);

  // Keep refs in sync with latest values on every render
  openTabsRef.current = openTabs;
  activeTabIdRef.current = activeTabId;
  activeProjectPathRef.current = activeProjectPath;

  const activeTab = useMemo(
    () => openTabs.find((tab) => tab.id === activeTabId) ?? null,
    [openTabs, activeTabId]
  );
  const activeFileTab = activeTab && isFileTab(activeTab) ? activeTab : null;
  const activeDiffTab = activeTab && isDiffTab(activeTab) ? activeTab : null;
  const activeChatTab = activeTab && isChatTab(activeTab) ? activeTab : null;

  // Cleanup on unmount
  useEffect(() => {
    return () => {
      if (autoSaveTimerRef.current !== null) {
        window.clearTimeout(autoSaveTimerRef.current);
      }
    };
  }, []);

  function clearAutoSaveTimer() {
    if (autoSaveTimerRef.current !== null) {
      window.clearTimeout(autoSaveTimerRef.current);
      autoSaveTimerRef.current = null;
    }
  }

  function replaceOpenFileTabContent(filePath: string, content: string) {
    const tabId = tabIdForPath(filePath);
    setOpenTabs((previous) =>
      previous.map((tab) =>
        tab.id === tabId && isFileTab(tab)
          ? { ...tab, content, savedContent: content, isLoading: false, isSaving: false }
          : tab
      )
    );
  }

  function closeOpenFileTab(filePath: string) {
    const tabId = tabIdForPath(filePath);
    setOpenTabs((previous) => previous.filter((tab) => tab.id !== tabId));
    setPendingLineJump((current) => (current?.tabId === tabId ? null : current));
    if (activeTabIdRef.current === tabId) {
      setActiveTabId(CONSOLE_TAB_ID);
    }
  }

  async function readFileSnapshot(projectPath: string, filePath: string): Promise<string | null> {
    try {
      const result = await client.request(PROVIDER_IDS.code, "code_readFile", [
        { projectPath, filePath },
      ]);
      return parseReadFileResult(result);
    } catch {
      return null;
    }
  }

  function openOrUpdateDiffTab(diffText: string, options: { activate?: boolean } = {}) {
    const nextTab = createDiffTab(diffText);
    setOpenTabs((previous) => {
      const existingIndex = previous.findIndex((tab) => tab.id === DIFF_TAB_ID);
      if (existingIndex === -1) return [...previous, nextTab];
      return previous.map((tab) => (tab.id === DIFF_TAB_ID ? nextTab : tab));
    });
    if (options.activate !== false) {
      setActiveTabId(DIFF_TAB_ID);
    }
  }

  function openLatestDiff() {
    if (lastChangeSet.length === 0) return;
    openOrUpdateDiffTab(buildUnifiedDiffForChanges(lastChangeSet), { activate: true });
  }

  async function saveFileTab(
    tabId: string,
    options: { announce?: boolean; silentError?: boolean } = {}
  ): Promise<{ error?: string; status?: string }> {
    const projectPath = activeProjectPathRef.current.trim();
    if (!projectPath) {
      if (!options.silentError) return { error: "Open a project before saving files." };
      return {};
    }

    const tab = openTabsRef.current.find((candidate) => candidate.id === tabId);
    if (!tab || !isFileTab(tab) || tab.isLoading || tab.isSaving) return {};

    if (!isFileTabDirty(tab)) {
      if (options.announce) return { status: `No changes to save for ${tab.path}` };
      return {};
    }

    const contentToSave = tab.content;
    setOpenTabs((previous) =>
      previous.map((candidate) =>
        candidate.id === tabId && isFileTab(candidate)
          ? { ...candidate, isSaving: true }
          : candidate
      )
    );

    try {
      await client.request(PROVIDER_IDS.code, "code_writeFile", [
        { projectPath, filePath: tab.path, content: contentToSave },
      ]);
      setOpenTabs((previous) =>
        previous.map((candidate) =>
          candidate.id === tabId && isFileTab(candidate)
            ? { ...candidate, savedContent: contentToSave, isSaving: false }
            : candidate
        )
      );
      return options.announce ? { status: `Saved ${tab.path}` } : {};
    } catch (error) {
      setOpenTabs((previous) =>
        previous.map((candidate) =>
          candidate.id === tabId && isFileTab(candidate)
            ? { ...candidate, isSaving: false }
            : candidate
        )
      );
      if (!options.silentError) {
        return { error: `Failed to save ${tab.path}: ${asErrorMessage(error)}` };
      }
      return {};
    }
  }

  async function maybeAutoSaveTab(tabId: string): Promise<void> {
    const tab = openTabsRef.current.find((candidate) => candidate.id === tabId);
    if (!tab || !isFileTab(tab) || !isFileTabDirty(tab) || tab.isSaving || tab.isLoading) return;
    await saveFileTab(tabId, { announce: false, silentError: true });
  }

  function scheduleAutoSave(tabId: string) {
    clearAutoSaveTimer();
    autoSaveTimerRef.current = window.setTimeout(() => {
      autoSaveTimerRef.current = null;
      void maybeAutoSaveTab(tabId);
    }, AUTO_SAVE_DEBOUNCE_MS);
  }

  async function activateTab(tabId: string): Promise<void> {
    if (tabId === activeTabIdRef.current) return;
    const previousTabId = activeTabIdRef.current;
    await maybeAutoSaveTab(previousTabId);
    clearAutoSaveTimer();
    setActiveTabId(tabId);
  }

  async function closeTab(tabId: string): Promise<void> {
    if (tabId === CONSOLE_TAB_ID || tabId === CHAT_TAB_ID) return;
    const tabsBeforeClose = openTabsRef.current;
    const index = tabsBeforeClose.findIndex((tab) => tab.id === tabId);
    if (index === -1) return;
    await maybeAutoSaveTab(tabId);
    clearAutoSaveTimer();
    const remaining = tabsBeforeClose.filter((tab) => tab.id !== tabId);
    const nextFallbackTab =
      remaining[index] ?? remaining[index - 1] ?? remaining[0] ?? createConsoleTab();
    setOpenTabs((previous) => previous.filter((tab) => tab.id !== tabId));
    setPendingLineJump((current) => (current?.tabId === tabId ? null : current));
    if (activeTabIdRef.current === tabId) {
      setActiveTabId(nextFallbackTab.id);
    }
  }

  async function openFileTab(
    filePath: string,
    options: {
      projectPath?: string;
      showStatus?: boolean;
      silentError?: boolean;
      activate?: boolean;
    } = {}
  ): Promise<boolean> {
    const projectPath = (options.projectPath ?? activeProjectPathRef.current).trim();
    const targetFilePath = filePath.trim();

    if (!projectPath) return false;
    if (!targetFilePath) return false;

    const tabId = tabIdForPath(targetFilePath);
    const existing = openTabsRef.current.find((tab) => tab.id === tabId);
    if (existing && isFileTab(existing)) {
      if (options.activate !== false) {
        await activateTab(tabId);
      }
      return true;
    }

    if (options.activate !== false) {
      await maybeAutoSaveTab(activeTabIdRef.current);
    }

    const loadingTab: FileTab = {
      id: tabId,
      kind: "file",
      path: targetFilePath,
      content: "",
      savedContent: "",
      isLoading: true,
      isSaving: false,
    };

    setOpenTabs((previous) => {
      if (previous.some((tab) => tab.id === tabId)) return previous;
      return [...previous, loadingTab];
    });

    if (options.activate !== false) {
      setActiveTabId(tabId);
    }

    try {
      const result = await client.request(PROVIDER_IDS.code, "code_readFile", [
        { projectPath, filePath: targetFilePath },
      ]);
      const content = parseReadFileResult(result);
      setOpenTabs((previous) =>
        previous.map((tab) =>
          tab.id === tabId && isFileTab(tab)
            ? { ...tab, content, savedContent: content, isLoading: false, isSaving: false }
            : tab
        )
      );
      return true;
    } catch (error) {
      setOpenTabs((previous) => previous.filter((tab) => tab.id !== tabId));
      if (activeTabIdRef.current === tabId) {
        setActiveTabId(CONSOLE_TAB_ID);
      }
      if (!options.silentError) {
        throw error;
      }
      return false;
    }
  }

  async function saveActiveTab(
    options: { announce?: boolean } = {}
  ): Promise<{ error?: string; status?: string }> {
    const tabId = activeTabIdRef.current;
    const tab = openTabsRef.current.find((candidate) => candidate.id === tabId);
    if (!tab || !isFileTab(tab)) return {};
    clearAutoSaveTimer();
    return saveFileTab(tabId, { announce: options.announce === true, silentError: false });
  }

  function handleActiveEditorChange(nextContent: string) {
    const tabId = activeTabIdRef.current;
    setOpenTabs((previous) =>
      previous.map((tab) =>
        tab.id === tabId && isFileTab(tab) ? { ...tab, content: nextContent } : tab
      )
    );
  }

  async function handleCodeFileChanged(value: unknown): Promise<void> {
    if (typeof value !== "object" || !value) return;
    const v = value as Record<string, unknown>;
    const changedPath = typeof v.path === "string" ? v.path.trim() : "";
    const changedKind = typeof v.kind === "string" ? v.kind.trim() : "";
    if (!changedPath || !changedKind) return;

    const tabId = tabIdForPath(changedPath);
    const openTab = openTabsRef.current.find((candidate) => candidate.id === tabId);
    if (!openTab || !isFileTab(openTab)) return;

    if (changedKind === "delete") {
      setOpenTabs((previous) => previous.filter((candidate) => candidate.id !== tabId));
      if (activeTabIdRef.current === tabId) {
        setActiveTabId(CONSOLE_TAB_ID);
      }
      return;
    }

    if (openTab.isSaving || isFileTabDirty(openTab)) return;

    const projectPath = activeProjectPathRef.current.trim();
    if (!projectPath) return;

    try {
      const result = await client.request(PROVIDER_IDS.code, "code_readFile", [
        { projectPath, filePath: changedPath },
      ]);
      const content = parseReadFileResult(result);
      setOpenTabs((previous) =>
        previous.map((candidate) => {
          if (candidate.id !== tabId || !isFileTab(candidate)) return candidate;
          if (candidate.isSaving || isFileTabDirty(candidate)) return candidate;
          return { ...candidate, content, savedContent: content, isLoading: false };
        })
      );
    } catch (error) {
      console_.append([
        `[system] failed to sync changed file ${changedPath}: ${asErrorMessage(error)}`,
      ]);
    }
  }

  async function openFileAtLocation(
    filePath: string,
    line: number
  ): Promise<{ error?: string; status?: string }> {
    const normalizedPath = normalizeConsolePathForProject(
      filePath,
      activeProjectPathRef.current
    );
    try {
      const opened = await openFileTab(normalizedPath, {
        showStatus: true,
        silentError: false,
        activate: true,
      });
      if (!opened) return {};
      setPendingLineJump({
        tabId: tabIdForPath(normalizedPath),
        line: Math.max(1, line),
        nonce: Date.now(),
      });
      return { status: `Opened file ${normalizedPath}` };
    } catch (error) {
      return { error: `Failed to read ${normalizedPath}: ${asErrorMessage(error)}` };
    }
  }

  function applyOpenedProject() {
    setOpenTabs([createConsoleTab(), createChatTab()]);
    setActiveTabId(CHAT_TAB_ID);
    setPendingLineJump(null);
    setLastChangeSet([]);
  }

  return {
    openTabs,
    openTabsRef,
    activeTabId,
    activeTabIdRef,
    activeTab,
    activeFileTab,
    activeDiffTab,
    activeChatTab,
    pendingLineJump,
    lastChangeSet,
    setLastChangeSet,
    setPendingLineJump,
    openFileTab,
    closeTab,
    activateTab,
    saveActiveTab,
    handleActiveEditorChange,
    scheduleAutoSave,
    clearAutoSaveTimer,
    replaceOpenFileTabContent,
    closeOpenFileTab,
    openOrUpdateDiffTab,
    openLatestDiff,
    handleCodeFileChanged,
    openFileAtLocation,
    applyOpenedProject,
    readFileSnapshot,
  };
}
