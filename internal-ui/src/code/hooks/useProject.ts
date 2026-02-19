import { useRef, useState } from "react";
import type { IpcClient } from "../../ipc/client";
import { PROVIDER_IDS } from "../../ipc/contracts";
import type { FileEntry, OpenProjectResult, ProjectSummary } from "../types";
import {
  asErrorMessage,
  parseListFilesResult,
  parseOpenProjectResult,
  parseProjectPath,
  parseProjectsResult,
} from "../utils";

export interface ProjectHook {
  projects: ProjectSummary[];
  loadingProjects: boolean;
  activeProjectPath: string;
  activeProjectPathRef: React.MutableRefObject<string>;
  fileTree: FileEntry[];
  expandedDirs: Set<string>;
  pendingAction: "create" | "open" | null;
  newProjectName: string;
  projectPathInput: string;
  openFilePathInput: string;
  setNewProjectName: (v: string) => void;
  setProjectPathInput: (v: string) => void;
  setOpenFilePathInput: (v: string) => void;
  setPendingAction: (v: "create" | "open" | null) => void;
  toggleDir: (path: string) => void;
  ensureDirExpanded: (filePath: string) => void;
  refreshFileTree: (pathOverride?: string, options?: { silent?: boolean }) => Promise<{ error?: string }>;
  loadProjects: () => Promise<{ error?: string }>;
  doOpenProject: (pathOverride?: string) => Promise<OpenProjectResult>;
  doCreateProject: (name: string) => Promise<OpenProjectResult>;
  setActiveProjectPath: (path: string) => void;
  setFileTree: (files: FileEntry[]) => void;
  setExpandedDirs: (dirs: Set<string>) => void;
}

export function useProject(client: IpcClient): ProjectHook {
  const [projects, setProjects] = useState<ProjectSummary[]>([]);
  const [loadingProjects, setLoadingProjects] = useState(true);
  const [pendingAction, setPendingAction] = useState<"create" | "open" | null>(null);
  const [newProjectName, setNewProjectName] = useState("");
  const [projectPathInput, setProjectPathInput] = useState("");
  const [openFilePathInput, setOpenFilePathInput] = useState("src/App.tsx");
  const [activeProjectPath, setActiveProjectPath] = useState("");
  const [fileTree, setFileTree] = useState<FileEntry[]>([]);
  const [expandedDirs, setExpandedDirs] = useState<Set<string>>(new Set());

  const activeProjectPathRef = useRef(activeProjectPath);
  activeProjectPathRef.current = activeProjectPath;

  function ensureDirExpanded(filePath: string) {
    const segments = filePath
      .split("/")
      .map((s) => s.trim())
      .filter(Boolean);
    if (segments.length < 2) return;
    setExpandedDirs((previous) => {
      const next = new Set(previous);
      let current = "";
      for (let i = 0; i < segments.length - 1; i += 1) {
        current = current ? `${current}/${segments[i]}` : segments[i];
        next.add(current);
      }
      return next;
    });
  }

  function toggleDir(path: string) {
    setExpandedDirs((previous) => {
      const next = new Set(previous);
      if (next.has(path)) {
        next.delete(path);
      } else {
        next.add(path);
      }
      return next;
    });
  }

  async function loadProjects(): Promise<{ error?: string }> {
    setLoadingProjects(true);
    try {
      const result = await client.request(PROVIDER_IDS.code, "code_listProjects", [{}]);
      setProjects(parseProjectsResult(result));
      return {};
    } catch (error) {
      setProjects([]);
      return { error: `Failed to load projects: ${asErrorMessage(error)}` };
    } finally {
      setLoadingProjects(false);
    }
  }

  async function refreshFileTree(
    pathOverride?: string,
    options: { silent?: boolean } = {}
  ): Promise<{ error?: string }> {
    const projectPath = (pathOverride ?? activeProjectPathRef.current).trim();
    if (!projectPath) return {};
    try {
      const result = await client.request(PROVIDER_IDS.code, "code_listFiles", [{ projectPath }]);
      setFileTree(parseListFilesResult(result));
      return {};
    } catch (error) {
      if (!options.silent) {
        return { error: `Failed to refresh file tree: ${asErrorMessage(error)}` };
      }
      return {};
    }
  }

  async function doOpenProject(pathOverride?: string): Promise<OpenProjectResult> {
    const path = (pathOverride ?? projectPathInput).trim();
    const params = path ? [{ path }] : [{}];
    const result = await client.request(PROVIDER_IDS.code, "code_openProject", params);
    const opened = parseOpenProjectResult(result);

    const initialExpanded = new Set<string>();
    opened.files.forEach((entry) => {
      if (entry.isDir) initialExpanded.add(entry.path);
    });

    setActiveProjectPath(opened.projectPath);
    setProjectPathInput(opened.projectPath);
    setFileTree(opened.files);
    setExpandedDirs(initialExpanded);
    return opened;
  }

  async function doCreateProject(name: string): Promise<OpenProjectResult> {
    const created = await client.request(PROVIDER_IDS.code, "code_createProject", [{ name }]);
    const projectPath = parseProjectPath(created, "code_createProject");
    const openedResult = await client.request(PROVIDER_IDS.code, "code_openProject", [
      { path: projectPath },
    ]);
    const opened = parseOpenProjectResult(openedResult);

    const initialExpanded = new Set<string>();
    opened.files.forEach((entry) => {
      if (entry.isDir) initialExpanded.add(entry.path);
    });

    setActiveProjectPath(opened.projectPath);
    setProjectPathInput(opened.projectPath);
    setFileTree(opened.files);
    setExpandedDirs(initialExpanded);
    return opened;
  }

  return {
    projects,
    loadingProjects,
    activeProjectPath,
    activeProjectPathRef,
    fileTree,
    expandedDirs,
    pendingAction,
    newProjectName,
    projectPathInput,
    openFilePathInput,
    setNewProjectName,
    setProjectPathInput,
    setOpenFilePathInput,
    setPendingAction,
    toggleDir,
    ensureDirExpanded,
    refreshFileTree,
    loadProjects,
    doOpenProject,
    doCreateProject,
    setActiveProjectPath,
    setFileTree,
    setExpandedDirs,
  };
}
