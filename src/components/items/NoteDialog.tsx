import { useEffect, useRef, useState } from "react";
import { useWindowStore } from "@/stores";
import { Button } from "@/components/ui/button";
import type { TrackedItem } from "@/stores/types";

interface Props {
  item: TrackedItem;
  onClose: () => void;
}

/** Small modal for annotating (renaming) a tracked resource. The note is
 *  saved per resource key, so it survives navigation and app restarts.
 *  Saving an empty value clears the note. Enter saves, Escape cancels. */
export function NoteDialog({ item, onClose }: Props) {
  const setResourceNote = useWindowStore((s) => s.setResourceNote);
  const [value, setValue] = useState(item.note ?? "");
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, []);

  const save = () => {
    setResourceNote(item.id, value);
    onClose();
  };

  return (
    <div
      className="fixed inset-0 z-50 bg-black/40 flex items-center justify-center"
      onClick={onClose}
    >
      <div
        className="bg-popover border rounded-lg shadow-xl p-4 w-[380px] max-w-[90vw]"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="text-sm font-medium mb-1">备注此页面</div>
        <div className="text-xs text-muted-foreground truncate mb-3">
          原名称：{item.title}
        </div>
        <input
          ref={inputRef}
          value={value}
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") save();
            if (e.key === "Escape") onClose();
          }}
          placeholder="输入备注名，显示时替代原标题"
          className="w-full h-9 px-3 text-sm rounded-md border bg-background focus:outline-none focus:ring-1 focus:ring-ring"
        />
        <div className="flex justify-end gap-2 mt-3">
          <Button variant="ghost" size="sm" onClick={onClose}>
            取消
          </Button>
          <Button size="sm" onClick={save}>
            保存
          </Button>
        </div>
      </div>
    </div>
  );
}
