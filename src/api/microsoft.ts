import { invoke } from "@tauri-apps/api/core";
import type { Account } from "./account";

/** 设备码登录的第一步：界面要把 code 放大显示，让用户去浏览器里敲 */
export interface MsDeviceLogin {
  user_code: string;
  verification_uri: string;
  message: string;
  /** 票据有效期（秒），过期得重新拿 */
  expires_in: number;
  /** 官方建议的轮询间隔（秒），太快会被判 slow_down */
  interval: number;
}

export type MsPollStatus = "pending" | "slow_down" | "success" | "denied" | "expired";

export interface MsPoll {
  status: MsPollStatus;
  message: string;
  /** 只有 success 时才有值 */
  account: Account | null;
}

export interface MsAccountState {
  id: string;
  /** 凭据库里还有 refresh token，即不用重新登录 */
  has_secret: boolean;
  /** 内存里有未过期的 Minecraft 令牌 */
  live: boolean;
}

export const microsoftApi = {
  start: () => invoke<MsDeviceLogin>("ms_login_start"),
  /** 每次只问一次，节奏由前端掌控，界面才随时能取消 */
  poll: () => invoke<MsPoll>("ms_login_poll"),
  state: (id: string) => invoke<MsAccountState>("ms_account_state", { id }),
  cancel: () => invoke<void>("ms_login_cancel"),
};

/**
 * 打开系统浏览器。走自家 `open_external` 命令，不依赖 opener 插件的权限白名单
 * （它的权限标识在 2.x 小版本里改过名，白名单漏一处就只在运行时才报错）。
 */
export async function openInBrowser(url: string) {
  await invoke<void>("open_external", { url });
}