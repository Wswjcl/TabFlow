import { useWindowStore } from "@/stores";
import type { ExtensionStatus } from "@/stores/types";
import { Card, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Globe, FolderOpen, Monitor, CopyX, Layers, RefreshCw, Wifi, WifiOff, Loader2, Puzzle, Copy, Check } from "lucide-react";
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface Props {
  onRefresh: () => Promise<void>;
  lastRefresh: string;
}

export function Dashboard({ onRefresh, lastRefresh }: Props) {
  const { stats, duplicates, loading, error } = useWindowStore();
  const dupCount = duplicates.reduce((s, g) => s + g.items.length - 1, 0);
  const [cdpOn, setCdpOn] = useState(false);
  const [launching, setLaunching] = useState(false);
  const [launchError, setLaunchError] = useState<string | null>(null);
  const [ext, setExt] = useState<ExtensionStatus | null>(null);
  const [pairingOpen, setPairingOpen] = useState(false);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    invoke<boolean>("check_cdp_status")
      .then(setCdpOn)
      .catch(() => setCdpOn(false));
    invoke<ExtensionStatus>("get_extension_status")
      .then(setExt)
      .catch(() => setExt(null));
  }, [lastRefresh]);

  const extConnected = ext != null && ext.connected.length > 0;

  const copyToken = async () => {
    if (!ext) return;
    try {
      await navigator.clipboard.writeText(ext.token);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      /* clipboard unavailable */
    }
  };

  const handleLaunchDebugBrowser = async () => {
    setLaunching(true);
    setLaunchError(null);
    try {
      await invoke<string>("launch_browser_debug");
      await onRefresh();
    } catch (e) {
      setLaunchError(String(e));
    } finally {
      setLaunching(false);
    }
  };

  return (
    <div className="px-4 py-3 border-b border-border/50 shrink-0">
      <div className="flex items-center justify-between mb-2">
        <div className="flex items-center gap-2">
          <span className="text-sm font-semibold">概览</span>
          {loading && (
            <span className="text-xs text-muted-foreground animate-pulse">...</span>
          )}
          {error && (
            <span className="text-xs text-destructive">加载失败</span>
          )}
          {cdpOn ? (
            <span
              className="text-[10px] flex items-center gap-1 text-green-500"
              title="CDP 已连接"
            >
              <Wifi className="w-3 h-3" />
              标签页模式
            </span>
          ) : (
            <button
              onClick={handleLaunchDebugBrowser}
              disabled={launching}
              title="浏览器需以调试模式启动才能读取标签页。注意：若浏览器已在运行，请先完全退出再点击。"
              className="text-[10px] flex items-center gap-1 text-muted-foreground/60 hover:text-foreground transition-colors disabled:opacity-50"
            >
              {launching ? (
                <Loader2 className="w-3 h-3 animate-spin" />
              ) : (
                <WifiOff className="w-3 h-3" />
              )}
              {launching ? "启动中…" : "窗口模式 · 点击启动调试浏览器"}
            </button>
          )}
          {extConnected ? (
            <span
              className="text-[10px] flex items-center gap-1 text-green-500 max-w-[200px] truncate"
              title={ext!.connected.map((c) => `${c.browser}: ${c.tabCount} 个标签`).join("\n")}
            >
              <Puzzle className="w-3 h-3 shrink-0" />
              扩展:{" "}
              {ext!.connected.map((c) => `${c.browser}(${c.tabCount})`).join(" · ")}
            </span>
          ) : (
            <button
              onClick={() => setPairingOpen(!pairingOpen)}
              title="安装 TabFlow 浏览器扩展后可实时获取标签页（无需调试模式）"
              className="text-[10px] flex items-center gap-1 text-muted-foreground/60 hover:text-foreground transition-colors"
            >
              <Puzzle className="w-3 h-3" />
              扩展未连接 · 配对
            </button>
          )}
          {launchError && (
            <span
              className="text-[10px] text-destructive max-w-[240px] truncate"
              title={launchError}
            >
              {launchError}
            </span>
          )}
          {lastRefresh && !loading && (
            <span className="text-[10px] text-muted-foreground/60">{lastRefresh}</span>
          )}
        </div>
        <Button
          variant="outline"
          size="sm"
          onClick={onRefresh}
          disabled={loading}
          className="gap-1.5"
        >
          <RefreshCw className={`w-3.5 h-3.5 ${loading ? "animate-spin" : ""}`} />
          刷新
        </Button>
      </div>

      {/* Extension pairing panel */}
      {pairingOpen && ext && !extConnected && (
        <div className="mb-2 flex items-center gap-2 flex-wrap text-[11px] text-muted-foreground bg-muted/50 rounded-md px-3 py-2">
          <Puzzle className="w-3.5 h-3.5 shrink-0" />
          <span>
            在浏览器中加载 <code>tabflow/extension</code> 扩展
            （扩展管理页 → 开发者模式 → 加载已解压的扩展程序），
            然后将下面的 Token 粘贴进扩展弹窗完成配对：
          </span>
          <code className="px-1.5 py-0.5 rounded bg-background border font-mono select-all">
            {ext.token}
          </code>
          <Button
            variant="outline"
            size="sm"
            className="h-6 px-2 text-[11px] gap-1"
            onClick={copyToken}
          >
            {copied ? <Check className="w-3 h-3" /> : <Copy className="w-3 h-3" />}
            {copied ? "已复制" : "复制"}
          </Button>
          <span className="ml-auto text-[10px] opacity-60">
            ws://127.0.0.1:{ext.port} · 数据仅本机传输
          </span>
        </div>
      )}

      <div className="flex items-center gap-3 overflow-x-auto pb-1">
        <MiniStat label="总窗口" value={stats?.total_items ?? 0} icon={<Layers className="w-3.5 h-3.5" />} />
        <MiniStat label="重复" value={dupCount} icon={<CopyX className="w-3.5 h-3.5" />} urgent={dupCount > 0} />
        <MiniStat label="浏览器" value={stats?.browser_tabs ?? 0} icon={<Globe className="w-3.5 h-3.5" />} />
        <MiniStat label="文件夹" value={stats?.explorer_windows ?? 0} icon={<FolderOpen className="w-3.5 h-3.5" />} />
        <MiniStat label="应用" value={stats?.app_windows ?? 0} icon={<Monitor className="w-3.5 h-3.5" />} />
      </div>
    </div>
  );
}

function MiniStat({
  label,
  value,
  icon,
  urgent,
}: {
  label: string;
  value: number;
  icon: React.ReactNode;
  urgent?: boolean;
}) {
  return (
    <Card className={`shrink-0 min-w-[80px] cursor-default ${urgent ? "border-destructive/40 bg-destructive/5" : ""}`}>
      <CardContent className="p-2 flex items-center gap-2">
        <span className={urgent ? "text-destructive" : "text-muted-foreground"}>{icon}</span>
        <div>
          <div className="text-lg font-bold leading-tight">{value}</div>
          <div className="text-[10px] text-muted-foreground">{label}</div>
        </div>
      </CardContent>
    </Card>
  );
}