import { useEffect, useState } from "react";
import type { Project } from "../../lib/types";
import { formatExplorerAddress } from "../../lib/fileExplorerPath";
import { ArrowUp, File, FolderPlus, Home, RefreshCw } from "../icons";

interface FileExplorerPathBarProps {
  project: Project;
  selectedPath: string;
  onNavigate: (value: string) => Promise<boolean>;
  onParent: () => void;
  onRoot: () => void;
  onRefresh: () => void;
  onNewFile: () => void;
  onNewFolder: () => void;
  parentLabel: string;
  rootLabel: string;
  refreshLabel: string;
  pathLabel: string;
  newFileLabel: string;
  newFolderLabel: string;
  invalidPathLabel: string;
  writable?: boolean;
}

export function FileExplorerPathBar({
  project,
  selectedPath,
  onNavigate,
  onParent,
  onRoot,
  onRefresh,
  onNewFile,
  onNewFolder,
  parentLabel,
  rootLabel,
  refreshLabel,
  pathLabel,
  newFileLabel,
  newFolderLabel,
  invalidPathLabel,
  writable = true,
}: FileExplorerPathBarProps) {
  const [value, setValue] = useState(formatExplorerAddress(project, selectedPath));
  const [invalid, setInvalid] = useState(false);

  useEffect(() => {
    setValue(formatExplorerAddress(project, selectedPath));
    setInvalid(false);
  }, [project.id, project.path, project.remote_path, project.environment_type, selectedPath]);

  const submit = async () => {
    const ok = await onNavigate(value);
    setInvalid(!ok);
  };

  return (
    <div className="ui-file-explorer-pathbar space-y-1.5">
      <div className="flex items-center gap-1">
        <button
          type="button"
          className="ui-file-tooltip ui-icon-action"
          data-tooltip={parentLabel}
          aria-label={parentLabel}
          disabled={!selectedPath}
          onClick={onParent}
        >
          <ArrowUp size={13} />
        </button>
        <button
          type="button"
          className="ui-file-tooltip ui-icon-action"
          data-tooltip={rootLabel}
          aria-label={rootLabel}
          onClick={onRoot}
        >
          <Home size={13} />
        </button>
        <button
          type="button"
          className="ui-file-tooltip ui-icon-action"
          data-tooltip={refreshLabel}
          aria-label={refreshLabel}
          onClick={onRefresh}
        >
          <RefreshCw size={13} />
        </button>
        {writable && (
          <>
            <button
              type="button"
              className="ui-file-tooltip ui-icon-action"
              data-tooltip={newFileLabel}
              aria-label={newFileLabel}
              onClick={onNewFile}
            >
              <File size={13} />
            </button>
            <button
              type="button"
              className="ui-file-tooltip ui-icon-action"
              data-tooltip={newFolderLabel}
              aria-label={newFolderLabel}
              onClick={onNewFolder}
            >
              <FolderPlus size={13} />
            </button>
          </>
        )}
      </div>
      <div className="ui-file-search-input-shell flex items-center rounded-md border border-border bg-surface-container-lowest px-1.5">
        <input
          className="min-w-0 flex-1 bg-transparent py-1 font-mono text-xs text-on-surface outline-none"
          value={value}
          aria-label={pathLabel}
          aria-invalid={invalid}
          spellCheck={false}
          onChange={(event) => {
            setValue(event.currentTarget.value);
            setInvalid(false);
          }}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              void submit();
            }
          }}
          onBlur={() => {
            if (value.trim() === formatExplorerAddress(project, selectedPath)) return;
            void submit();
          }}
        />
      </div>
      {invalid && <div className="px-0.5 text-[10px] text-danger">{invalidPathLabel}</div>}
    </div>
  );
}
