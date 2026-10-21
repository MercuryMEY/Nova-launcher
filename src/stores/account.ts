import { defineStore } from "pinia";
import { computed, ref } from "vue";
import {
  accountApi,
  errorText as accountErrorText,
  type Account,
  type SecretStatus,
  type UuidMode,
} from "../api/account";
import { microsoftApi, type MsDeviceLogin } from "../api/microsoft";
import { useNotifyStore } from "./notify";

export const useAccountStore = defineStore("account", () => {
  const accounts = ref<Account[]>([]);
  const secretStatus = ref<SecretStatus | null>(null);
  const loading = ref(false);
  const busy = ref(false);
  const current = computed(() => accounts.value.find((a) => a.is_current) ?? null);

  async function load() {
    if (loading.value) return;
    loading.value = true;
    try {
      accounts.value = await accountApi.list();
      secretStatus.value = await accountApi.secretStatus();
    } catch (e) {
      useNotifyStore().error(accountErrorText(e));
    } finally {
      loading.value = false;
    }
  }

  async function refresh() {
    accounts.value = await accountApi.list();
  }

  /** 所有写操作都走这里，统一 busy 状态与错误提示 */
  async function run<T>(task: () => Promise<T>): Promise<T | null> {
    if (busy.value) return null;
    const notify = useNotifyStore();
    busy.value = true;
    try {
      const result = await task();
      await refresh();
      return result;
    } catch (e) {
      notify.error(accountErrorText(e));
      return null;
    } finally {
      busy.value = false;
    }
  }

  async function add(name: string, mode: UuidMode) {
    const notify = useNotifyStore();
    const created = await run(() => accountApi.add(name, mode));
    if (created) notify.success(`已添加离线账户「${created.name}」`);
    return created;
  }

  async function update(id: string, name: string, mode: UuidMode) {
    const notify = useNotifyStore();
    const saved = await run(() => accountApi.update(id, name, mode));
    if (saved) notify.success("账户信息已更新");
    return saved;
  }

  async function remove(id: string) {
    const notify = useNotifyStore();
    // run 失败返回 null，这里用 true 标记成功，避免 void 与 null 比较
    const ok = await run(() => accountApi.remove(id).then(() => true));
    if (ok) notify.success("账户已删除");
    return ok === true;
  }

  async function setCurrent(id: string) {
    const notify = useNotifyStore();
    const ok = await run(() => accountApi.setCurrent(id).then(() => true));
    if (ok) notify.success("已切换当前账户");
    return ok === true;
  }

  // ---------- 微软登录（设备码） ----------

  /** 正在等确认的那张票据；null 表示没有进行中的登录 */
  const msTicket = ref<MsDeviceLogin | null>(null);
  /** 给界面看的一句话状态，来自后端每一步的反馈 */
  const msMessage = ref("");
  /** 一次轮询正在进行，避免定时器叠起来一起打后端 */
  const msPolling = ref(false);
  /** 刚登录成功的账户 id，界面用它把选中项切过去 */
  const msDoneId = ref<string | null>(null);

  let msTimer: ReturnType<typeof setTimeout> | null = null;

  function stopMsTimer() {
    if (msTimer !== null) {
      clearTimeout(msTimer);
      msTimer = null;
    }
  }

  function schedulePoll(seconds: number) {
    stopMsTimer();
    // 下限 3 秒：微软对“问得太勤”会直接回 slow_down
    const wait = Math.max(3, seconds) * 1000;
    msTimer = setTimeout(() => {
      msTimer = null;
      void pollMicrosoft();
    }, wait);
  }

  async function pollMicrosoft() {
    if (!msTicket.value) return;
    // 上一次还没回来就撞上了下一次：让它先完成，别丢掉既定的轮询节奏
    if (msPolling.value) {
      schedulePoll(msTicket.value.interval);
      return;
    }
    msPolling.value = true;
    const notify = useNotifyStore();
    try {
      const res = await microsoftApi.poll();
      msMessage.value = res.message;
      if (res.status === "success") {
        stopMsTimer();
        msTicket.value = null;
        await refresh();
        msDoneId.value = res.account?.id ?? null;
        notify.success(res.message || "微软账户已登录");
        return;
      }
      if (res.status === "denied" || res.status === "expired") {
        stopMsTimer();
        msTicket.value = null;
        notify.error(res.message);
        return;
      }
      // pending / slow_down 都继续等；slow_down 就按后端建议再拉长一点
      const base = msTicket.value.interval;
      schedulePoll(res.status === "slow_down" ? base + 2 : base);
    } catch (e) {
      stopMsTimer();
      msTicket.value = null;
      notify.error(accountErrorText(e));
    } finally {
      msPolling.value = false;
    }
  }

  async function startMicrosoft() {
    if (msTicket.value) return;
    const notify = useNotifyStore();
    try {
      const ticket = await microsoftApi.start();
      msTicket.value = ticket;
      msMessage.value = ticket.message;
      msDoneId.value = null;
      schedulePoll(ticket.interval);
    } catch (e) {
      notify.error(accountErrorText(e));
    }
  }

  /** 用户放弃：先停本地定时器，再让后端把票据丢掉 */
  async function cancelMicrosoft() {
    stopMsTimer();
    msTicket.value = null;
    msMessage.value = "";
    try {
      await microsoftApi.cancel();
    } catch {
      // 取消不该弹错，失败最多是后端票据多留一会儿
    }
  }

  /** 重新问后端“这号还能不能直接用”，用来决定要不要提示重新登录 */
  async function msState(id: string) {
    try {
      return await microsoftApi.state(id);
    } catch {
      return { id, has_secret: false, live: false };
    }
  }

  return {
    accounts,
    current,
    secretStatus,
    loading,
    busy,
    load,
    refresh,
    add,
    update,
    remove,
    setCurrent,
    msTicket,
    msMessage,
    msPolling,
    msDoneId,
    startMicrosoft,
    cancelMicrosoft,
    msState,
  };
});