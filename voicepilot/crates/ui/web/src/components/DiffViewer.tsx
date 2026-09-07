import { useEffect, useState } from "react";
import { computeDiff } from "../api";
import type { DiffResult } from "../types";

interface Props {
  sourcePath: string;
  destPath: string;
  onClose: () => void;
}

export function DiffViewer({ sourcePath, destPath, onClose }: Props): JSX.Element {
  const [result, setResult] = useState<DiffResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    computeDiff(sourcePath, destPath)
      .then((r) => {
        if (!cancelled) {
          setResult(r);
          setError(null);
        }
      })
      .catch((e: unknown) => {
        if (!cancelled) setError(String(e));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [sourcePath, destPath]);

  return (
    <div className="diff-card" role="dialog" aria-label="文件 Diff 预览">
      <div className="card-title head-gap">
        <span className="tick" aria-hidden="true" />
        文件 Diff
        <span className="flex-spacer" />
        <button
          type="button"
          className="icon-btn icon-btn-sm"
          onClick={onClose}
          aria-label="关闭 Diff"
        >
          ×
        </button>
      </div>
      <div className="diff-paths">
        <div><span className="field-label">源：</span>{sourcePath}</div>
        <div><span className="field-label">目标：</span>{destPath}</div>
      </div>
      {loading && <p className="diff-note">加载中…</p>}
      {error && <p className="diff-note diff-error">错误：{error}</p>}
      {result && !loading && !error && <DiffContent result={result} />}
    </div>
  );
}

function DiffContent({ result }: { result: DiffResult }): JSX.Element {
  if (result.truncated) {
    return (
      <p className="diff-note" role="status">
        ⚠ {result.truncate_reason}
      </p>
    );
  }
  if (result.file_kind === "binary") {
    return <p className="diff-note">二进制文件，不展示 diff</p>;
  }
  if (result.file_kind === "new_file" && result.diff_text) {
    return (
      <pre className="diff-text">
        <code>{result.diff_text}</code>
      </pre>
    );
  }
  return (
    <pre className="diff-text">
      <code>{result.diff_text ?? "（无差异）"}</code>
    </pre>
  );
}
