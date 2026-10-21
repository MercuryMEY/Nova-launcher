import { defineStore } from "pinia";
import { ref } from "vue";

export type NotifyKind = "info" | "success" | "error";

export interface Notice {
  id: number;
  kind: NotifyKind;
  text: string;
}

let seq = 0;

export const useNotifyStore = defineStore("notify", () => {
  const items = ref<Notice[]>([]);

  function push(kind: NotifyKind, text: string, ttl = 3600) {
    // 合并重复消息：已存在同样文案则只刷新类型
    const exist = items.value.find((n) => n.text === text);
    if (exist) {
      exist.kind = kind;
      return;
    }
    const id = ++seq;
    items.value.push({ id, kind, text });
    window.setTimeout(() => dismiss(id), ttl);
  }

  function dismiss(id: number) {
    const i = items.value.findIndex((n) => n.id === id);
    if (i >= 0) items.value.splice(i, 1);
  }

  return {
    items,
    info: (t: string) => push("info", t),
    success: (t: string) => push("success", t),
    error: (t: string) => push("error", t, 6000),
    dismiss,
  };
});
