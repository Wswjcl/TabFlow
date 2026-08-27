import { useWindowStore } from "@/stores";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import { EyeOff, RotateCcw, Sparkles } from "lucide-react";

/** Management view for ignored resources: each row can be unignored,
 *  which puts the resource back into normal tracking. */
export function IgnoredList() {
  const { ignored, unignoreResource } = useWindowStore();

  if (ignored.length === 0) {
    return (
      <div className="flex-1 flex flex-col items-center justify-center text-muted-foreground gap-2 p-8">
        <Sparkles className="w-8 h-8 opacity-30" />
        <span className="text-sm">没有忽略的资源</span>
        <span className="text-xs opacity-50 text-center max-w-xs">
          右键任意窗口或标签页，选择「忽略追踪」，它就会从这里消失
        </span>
      </div>
    );
  }

  return (
    <ScrollArea className="flex-1 p-4">
      <div className="flex items-center gap-2 mb-3">
        <span className="text-sm font-semibold text-muted-foreground">
          已忽略 ({ignored.length})
        </span>
        <span className="text-xs text-muted-foreground/60">
          恢复后会重新参与统计与重复检测
        </span>
      </div>

      <div className="space-y-1">
        {ignored.map((res) => (
          <div
            key={res.resource_key}
            className="group flex items-center gap-2 px-3 py-2 rounded-md hover:bg-accent/70 transition-colors"
          >
            <EyeOff className="w-4 h-4 shrink-0 text-muted-foreground/50" />

            <div className="flex-1 min-w-0">
              <div className="text-sm font-medium truncate">{res.title || res.resource_key}</div>
              <div className="text-xs text-muted-foreground/60 truncate" title={res.resource_key}>
                {res.resource_key}
              </div>
            </div>

            <Button
              variant="ghost"
              size="sm"
              className="h-7 px-2 text-xs shrink-0 opacity-0 group-hover:opacity-100 transition-opacity"
              onClick={() => unignoreResource(res.resource_key)}
              title="恢复追踪"
            >
              <RotateCcw className="w-3 h-3 mr-1" />
              恢复追踪
            </Button>
          </div>
        ))}
      </div>
    </ScrollArea>
  );
}
