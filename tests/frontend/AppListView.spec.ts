import { flushPromises, mount } from "@vue/test-utils";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import AppListView from "../../src/components/AppListView.vue";
import { useI18n } from "../../src/i18n";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn()
}));

interface AppCard {
  id: string;
  name: string;
  icon: string;
  iconPath: string | null;
  size: string;
  sizeLabel?: string;
  isMigrated: boolean;
  targetDisk: string | null;
  path: string;
  paths?: string[];
  pathGroups?: {
    key: string;
    label: string;
    paths: string[];
    entries?: {
      path: string;
      displayName?: string;
      migrated: boolean;
      pending: boolean;
    }[];
  }[];
  pendingPathCount?: number;
  migratedPathCount?: number;
  desc: string;
  availability: "active" | "blocked" | "deprecated";
  blockedReason?: string | null;
  requiresConfirmation?: boolean;
  hasExecutableUnit?: boolean;
  running: boolean;
}

describe("AppListView", () => {
  const invokeMock = vi.mocked(invoke);
  const clipboardWriteText = vi.fn();

  beforeEach(() => {
    window.localStorage.clear();
    useI18n().setLocale("zh");
    invokeMock.mockReset();
    clipboardWriteText.mockReset();
    clipboardWriteText.mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText: clipboardWriteText },
      configurable: true
    });
  });

  it("emits refresh/migrate/restore and disables blocked/running migration", async () => {
    const apps: AppCard[] = [
      {
        id: "wechat",
        name: "WeChat",
        icon: "💬",
        iconPath: null,
        size: "10 GB",
        isMigrated: false,
        targetDisk: null,
        path: "~/Library/Containers/com.tencent.xinWeChat",
        desc: "desc",
        availability: "active",
        running: false
      },
      {
        id: "blocked-app",
        name: "Blocked",
        icon: "📦",
        iconPath: null,
        size: "1 GB",
        isMigrated: false,
        targetDisk: null,
        path: "~/blocked",
        desc: "desc",
        availability: "blocked",
        running: false
      },
      {
        id: "running-app",
        name: "Running",
        icon: "📦",
        iconPath: null,
        size: "2 GB",
        isMigrated: false,
        targetDisk: null,
        path: "~/running",
        desc: "desc",
        availability: "active",
        running: true
      },
      {
        id: "telegram",
        name: "Telegram",
        icon: "✈️",
        iconPath: null,
        size: "5 GB",
        isMigrated: true,
        targetDisk: "M4_Ext_SSD",
        path: "~/Library/Group Containers",
        desc: "desc",
        availability: "active",
        running: false
      }
    ];

    const wrapper = mount(AppListView, {
      props: {
        apps,
        loading: false,
        error: null
      }
    });

    const refreshBtn = wrapper
      .findAll("button")
      .find((btn) => btn.text().includes("刷新扫描"));
    expect(refreshBtn).toBeDefined();
    await refreshBtn!.trigger("click");
    expect(wrapper.emitted("refresh")).toHaveLength(1);

    const migrateButtons = wrapper
      .findAll("button")
      .filter((btn) => btn.text().includes("搬迁外存"));
    expect(migrateButtons).toHaveLength(3);
    expect(migrateButtons[0].attributes("disabled")).toBeUndefined();
    expect(migrateButtons[1].attributes("disabled")).toBeDefined();
    expect(migrateButtons[2].attributes("disabled")).toBeDefined();

    await migrateButtons[0].trigger("click");
    expect(wrapper.emitted("migrate")?.[0]).toEqual(["wechat"]);

    const restoreBtn = wrapper
      .findAll("button")
      .find((btn) => btn.text().includes("恢复到系统"));
    expect(restoreBtn).toBeDefined();
    await restoreBtn!.trigger("click");
    expect(wrapper.emitted("restore")?.[0]).toEqual(["telegram"]);
  });

  it("falls back to emoji icon when image loading fails", async () => {
    const apps: AppCard[] = [
      {
        id: "wechat",
        name: "WeChat",
        icon: "💬",
        iconPath: "/tmp/non-existent-icon.png",
        size: "10 GB",
        isMigrated: false,
        targetDisk: null,
        path: "~/Library/Containers/com.tencent.xinWeChat",
        desc: "desc",
        availability: "active",
        running: false
      }
    ];

    const wrapper = mount(AppListView, {
      props: {
        apps,
        loading: false,
        error: null
      }
    });

    expect(wrapper.find("img").exists()).toBe(true);
    await wrapper.find("img").trigger("error");
    expect(wrapper.find("img").exists()).toBe(false);
    expect(wrapper.text()).toContain("💬");
  });

  it("shows empty prompt when app list is empty and not loading", () => {
    const wrapper = mount(AppListView, {
      props: {
        apps: [],
        loading: false,
        error: null
      }
    });
    expect(wrapper.text()).toContain("未检测到可识别应用");
  });

  it("renders blocked and running hints correctly", () => {
    const wrapper = mount(AppListView, {
      props: {
        apps: [
          {
            id: "blocked-app",
            name: "Blocked",
            icon: "📦",
            iconPath: null,
            size: "1 GB",
            isMigrated: false,
            targetDisk: null,
            path: "~/blocked",
            desc: "desc",
            availability: "blocked",
            running: false
          },
          {
            id: "running-app",
            name: "Running",
            icon: "📦",
            iconPath: null,
            size: "1 GB",
            isMigrated: false,
            targetDisk: null,
            path: "~/running",
            desc: "desc",
            availability: "active",
            running: true
          }
        ],
        loading: false,
        error: null
      }
    });

    expect(wrapper.text()).toContain("当前画像为 blocked，不支持迁移");
    expect(wrapper.text()).toContain("应用正在运行，请先完全退出后再执行迁移/恢复");
  });

  it("shows refreshing label and disables refresh button while loading", () => {
    const wrapper = mount(AppListView, {
      props: {
        apps: [],
        loading: true,
        error: null
      }
    });

    const refreshBtn = wrapper
      .findAll("button")
      .find((btn) => btn.text().includes("刷新中"));
    expect(refreshBtn).toBeDefined();
    expect(refreshBtn!.attributes("disabled")).toBeDefined();
  });

  it("shows migrated fallback label when target disk name is missing", () => {
    const wrapper = mount(AppListView, {
      props: {
        apps: [
          {
            id: "migrated-no-disk",
            name: "Migrated",
            icon: "📦",
            iconPath: null,
            size: "3 GB",
            isMigrated: true,
            targetDisk: null,
            path: "~/migrated",
            desc: "desc",
            availability: "active",
            running: false
          }
        ],
        loading: false,
        error: null
      }
    });

    expect(wrapper.text()).toContain("已外存");
    expect(wrapper.text()).not.toContain("已外存至");
  });

  it("shows confirmation badge and hint for risky app", () => {
    const wrapper = mount(AppListView, {
      props: {
        apps: [
          {
            id: "risk-app",
            name: "Risky App",
            icon: "🧪",
            iconPath: null,
            size: "1 GB",
            isMigrated: false,
            targetDisk: null,
            path: "~/risk",
            desc: "desc",
            availability: "active",
            requiresConfirmation: true,
            running: false
          }
        ],
        loading: false,
        error: null
      }
    });

    expect(wrapper.text()).toContain("需确认");
    expect(wrapper.text()).toContain("包含需确认的数据单元，迁移前请确认风险");
  });

  it("disables migration for deprecated app and app without executable units", () => {
    const wrapper = mount(AppListView, {
      props: {
        apps: [
          {
            id: "deprecated-app",
            name: "Deprecated App",
            icon: "📦",
            iconPath: null,
            size: "1 GB",
            isMigrated: false,
            targetDisk: null,
            path: "~/deprecated",
            desc: "desc",
            availability: "deprecated",
            running: false
          },
          {
            id: "no-unit-app",
            name: "No Unit App",
            icon: "📦",
            iconPath: null,
            size: "1 GB",
            isMigrated: false,
            targetDisk: null,
            path: "~/no-unit",
            desc: "desc",
            availability: "active",
            hasExecutableUnit: false,
            running: false
          }
        ],
        loading: false,
        error: null
      }
    });

    const migrateButtons = wrapper
      .findAll("button")
      .filter((btn) => btn.text().includes("搬迁外存"));
    expect(migrateButtons).toHaveLength(2);
    expect(migrateButtons[0].attributes("disabled")).toBeDefined();
    expect(migrateButtons[1].attributes("disabled")).toBeDefined();
    expect(wrapper.text()).toContain("当前画像已弃用，默认不支持新迁移");
    expect(wrapper.text()).toContain("当前没有可迁移的数据单元");
  });

  it("shows both migrate and restore actions for partially migrated app", async () => {
    const wrapper = mount(AppListView, {
      props: {
        apps: [
          {
            id: "partial-app",
            name: "Partial",
            icon: "📦",
            iconPath: null,
            size: "1 GB",
            isMigrated: false,
            targetDisk: "M4_Ext_SSD",
            path: "/Users/test/partial",
            desc: "desc",
            availability: "active",
            migratedPathCount: 1,
            pendingPathCount: 1,
            running: false
          }
        ],
        loading: false,
        error: null
      }
    });

    const migrateBtn = wrapper.findAll("button").find((btn) => btn.text().includes("搬迁外存"));
    const restoreBtn = wrapper.findAll("button").find((btn) => btn.text().includes("恢复到系统"));
    expect(migrateBtn).toBeDefined();
    expect(restoreBtn).toBeDefined();

    await migrateBtn!.trigger("click");
    await restoreBtn!.trigger("click");
    expect(wrapper.emitted("migrate")?.[0]).toEqual(["partial-app"]);
    expect(wrapper.emitted("restore")?.[0]).toEqual(["partial-app"]);
  });

  it("disables restore action when migrated app is still running", async () => {
    const wrapper = mount(AppListView, {
      props: {
        apps: [
          {
            id: "running-migrated-app",
            name: "Running Migrated",
            icon: "📦",
            iconPath: null,
            size: "1 GB",
            isMigrated: true,
            targetDisk: "M4_Ext_SSD",
            path: "/Users/test/running-migrated",
            desc: "desc",
            availability: "active",
            running: true
          }
        ],
        loading: false,
        error: null
      }
    });

    const restoreBtn = wrapper.findAll("button").find((btn) => btn.text().includes("恢复到系统"));
    expect(restoreBtn).toBeDefined();
    expect(restoreBtn!.attributes("disabled")).toBeDefined();
    await restoreBtn!.trigger("click");
    expect(wrapper.emitted("restore")).toBeUndefined();
  });

  it("renders upstream error message when refresh fails", () => {
    const wrapper = mount(AppListView, {
      props: {
        apps: [],
        loading: false,
        error: "数据加载失败：scan failed"
      }
    });

    expect(wrapper.text()).toContain("数据加载失败：scan failed");
  });

  it("shows only directory summary on app card and keeps path actions in details dialog", () => {
    const wrapper = mount(AppListView, {
      props: {
        apps: [
          {
            id: "wechat",
            name: "WeChat",
            icon: "💬",
            iconPath: null,
            size: "10 GB",
            isMigrated: false,
            targetDisk: null,
            path: "/Users/test/Library/Containers/com.tencent.xinWeChat",
            desc: "desc",
            availability: "active",
            running: false
          }
        ],
        loading: false,
        error: null
      }
    });

    expect(wrapper.find('[data-test="app-path-details-btn"]').exists()).toBe(true);
    expect(wrapper.findAll('[data-test="app-open-path-btn"]')).toHaveLength(0);
    expect(wrapper.findAll('[data-test="app-copy-path-btn"]')).toHaveLength(0);
  });

  it("renders grouped directories by account and supports per-path actions", async () => {
    invokeMock.mockResolvedValue(undefined);

    const wrapper = mount(AppListView, {
      props: {
        apps: [
          {
            id: "wechat",
            name: "WeChat",
            icon: "💬",
            iconPath: null,
            size: "10 GB",
            isMigrated: false,
            targetDisk: null,
            path: "/Users/test/Library/Containers/com.tencent.xinWeChat/Data/Documents/xwechat_files/wxid_a/msg",
            paths: [
              "/Users/test/Library/Containers/com.tencent.xinWeChat/Data/Documents/xwechat_files/wxid_a/msg",
              "/Users/test/Library/Containers/com.tencent.xinWeChat/Data/Documents/xwechat_files/wxid_a/fav",
              "/Users/test/Library/Containers/com.tencent.xinWeChat/Data/Documents/xwechat_files/wxid_b/msg"
            ],
            pathGroups: [
              {
                key: "wxid_a",
                label: "账号 wxid_a",
                paths: [
                  "/Users/test/Library/Containers/com.tencent.xinWeChat/Data/Documents/xwechat_files/wxid_a/msg",
                  "/Users/test/Library/Containers/com.tencent.xinWeChat/Data/Documents/xwechat_files/wxid_a/fav"
                ],
                entries: [
                  {
                    path: "/Users/test/Library/Containers/com.tencent.xinWeChat/Data/Documents/xwechat_files/wxid_a/msg",
                    displayName: "聊天媒体资源库 [wxid_a]",
                    migrated: true,
                    pending: false
                  },
                  {
                    path: "/Users/test/Library/Containers/com.tencent.xinWeChat/Data/Documents/xwechat_files/wxid_a/fav",
                    displayName: "收藏附件 [wxid_a]",
                    migrated: false,
                    pending: true
                  }
                ]
              },
              {
                key: "wxid_b",
                label: "账号 wxid_b",
                paths: [
                  "/Users/test/Library/Containers/com.tencent.xinWeChat/Data/Documents/xwechat_files/wxid_b/msg"
                ],
                entries: [
                  {
                    path: "/Users/test/Library/Containers/com.tencent.xinWeChat/Data/Documents/xwechat_files/wxid_b/msg",
                    displayName: "聊天媒体资源库 [wxid_b]",
                    migrated: false,
                    pending: true
                  }
                ]
              }
            ],
            pendingPathCount: 2,
            desc: "desc",
            availability: "active",
            running: false
          }
        ],
        loading: false,
        error: null
      }
    });

    expect(wrapper.text()).toContain("3 个目录");
    expect(wrapper.text()).toContain("查看目录详情");
    expect(wrapper.text()).toContain("待迁移 2");

    const detailsBtn = wrapper.find('[data-test="app-path-details-btn"]');
    expect(detailsBtn.exists()).toBe(true);
    await detailsBtn.trigger("click");
    await flushPromises();

    const modal = wrapper.find('[data-test="app-path-details-modal"]');
    expect(modal.exists()).toBe(true);
    expect(modal.text()).toContain("账号 wxid_a");
    expect(modal.text()).toContain("账号 wxid_b");
    expect(modal.text()).toContain("已迁移");
    expect(modal.text()).toContain("未迁移");
    expect(modal.text()).toContain("聊天媒体资源库");
    expect(modal.text()).toContain("收藏附件");
    expect(modal.text()).not.toContain("[wxid_a]");
    expect(modal.text()).not.toContain("[wxid_b]");

    const accountBTab = modal.findAll("button").find((btn) => btn.text().includes("账号 wxid_b"));
    expect(accountBTab).toBeDefined();
    await accountBTab!.trigger("click");
    await flushPromises();

    const openButtons = modal.findAll('[data-test="app-open-path-btn"]');
    expect(openButtons).toHaveLength(1);
    await openButtons[0].trigger("click");
    await flushPromises();
    expect(invokeMock).toHaveBeenCalledWith("open_in_finder", {
      path: "/Users/test/Library/Containers/com.tencent.xinWeChat/Data/Documents/xwechat_files/wxid_b/msg"
    });

    const copyButtons = modal.findAll('[data-test="app-copy-path-btn"]');
    expect(copyButtons).toHaveLength(1);
    await copyButtons[0].trigger("click");
    await flushPromises();
    expect(clipboardWriteText).toHaveBeenCalledWith(
      "/Users/test/Library/Containers/com.tencent.xinWeChat/Data/Documents/xwechat_files/wxid_b/msg"
    );
  });

  it("supports copy path and open in Finder actions in path details dialog", async () => {
    invokeMock.mockResolvedValue(undefined);

    const wrapper = mount(AppListView, {
      props: {
        apps: [
          {
            id: "wechat",
            name: "WeChat",
            icon: "💬",
            iconPath: null,
            size: "10 GB",
            isMigrated: false,
            targetDisk: null,
            path: "/Users/test/Library/Containers/com.tencent.xinWeChat",
            desc: "desc",
            availability: "active",
            running: false
          }
        ],
        loading: false,
        error: null
      }
    });

    const detailsBtn = wrapper.find('[data-test="app-path-details-btn"]');
    expect(detailsBtn.exists()).toBe(true);
    await detailsBtn.trigger("click");
    await flushPromises();

    const modal = wrapper.find('[data-test="app-path-details-modal"]');
    expect(modal.exists()).toBe(true);

    const openBtn = modal.find('[data-test="app-open-path-btn"]');
    expect(openBtn.exists()).toBe(true);
    await openBtn.trigger("click");
    await flushPromises();
    expect(invokeMock).toHaveBeenCalledWith("open_in_finder", {
      path: "/Users/test/Library/Containers/com.tencent.xinWeChat"
    });

    const copyBtn = modal.find('[data-test="app-copy-path-btn"]');
    expect(copyBtn.exists()).toBe(true);
    await copyBtn.trigger("click");
    await flushPromises();
    expect(clipboardWriteText).toHaveBeenCalledWith(
      "/Users/test/Library/Containers/com.tencent.xinWeChat"
    );
    expect(copyBtn.text()).toBe("已复制");
  });
});
