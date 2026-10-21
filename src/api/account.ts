import { invoke } from "@tauri-apps/api/core";

export type UuidMode = "name-based" | "random";

export interface Account {
  id: string;
  kind: "microsoft" | "offline";
  name: string;
  uuid: string;
  uuid_mode: UuidMode;
  is_current: boolean;
  /** 微软账户：凭据库里还有 refresh token，不用重新登录 */
  has_secret: boolean;
}

export interface SecretStatus {
  backend: string;
  backend_name: string;
  available: boolean;
  degraded: boolean;
  note: string;
}

/** 复用版本页那套错误归一化，保证提示文案只在一个地方维护 */
export { errorText } from "./version";

export const accountApi = {
  list: () => invoke<Account[]>("list_accounts"),
  add: (name: string, uuidMode: UuidMode) =>
    invoke<Account>("add_offline_account", { name, uuidMode }),
  update: (id: string, name: string, uuidMode: UuidMode) =>
    invoke<Account>("update_offline_account", { id, name, uuidMode }),
  remove: (id: string) => invoke<void>("remove_account", { id }),
  setCurrent: (id: string | null) => invoke<void>("set_current_account", { id }),
  secretStatus: () => invoke<SecretStatus>("secret_store_status"),
};
