import { useWindowStore } from "@/stores";
import { Card, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Globe, FolderOpen, Monitor, CopyX, Layers, RefreshCw, Wifi, WifiOff } from "lucide-react";
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

  useEffect(() => {
    invoke<boolean>("check_cdp_status")
      .then(setCdpOn)
      .catch(() => setCdpOn(false));
  }, [lastRefresh]);

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
          <span
            className={`text-[10px] flex items-center gap-1 ${cdpOn ? "text-green-500" : "text-muted-foreground/40"}`}
            title={cdpOn ? "CDP 已连接" : "CDP 未连接 — 浏览器需以调试模式启动"}
          >
            {cdpOn ? <Wifi className="w-3 h-3" /> : <WifiOff className="w-3 h-3" />}
            {cdpOn ? "标签页模式" : "窗口模式"}
          </span>
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