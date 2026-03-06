import { mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it } from "vitest";
import AppListView from "../../src/components/AppListView.vue";
import { useI18n } from "../../src/i18n";

interface AppCard {
  id: string;
  name: string;
  icon: string;
  iconPath: string | null;
  size: string;
  isMigrated: boolean;
  targetDisk: string | null;
  path: string;
  desc: string;
  tier: "supported" | "experimental" | "blocked";
  running: boolean;
}

describe("AppListView", () => {
  beforeEach(() => {
    window.localStorage.clear();
    useI18n().setLocale("zh");
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
        tier: "supported",
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
        tier: "blocked",
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
        tier: "supported",
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
        tier: "supported",
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
        tier: "supported",
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
            tier: "blocked",
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
            tier: "supported",
            running: true
          }
        ],
        loading: false,
        error: null
      }
    });

    expect(wrapper.text()).toContain("当前画像为 blocked，不支持迁移");
    expect(wrapper.text()).toContain("应用正在运行，请先完全退出再迁移");
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
            tier: "supported",
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

  it("shows experimental badge and hint for experimental app", () => {
    const wrapper = mount(AppListView, {
      props: {
        apps: [
          {
            id: "exp-app",
            name: "Experimental App",
            icon: "🧪",
            iconPath: null,
            size: "1 GB",
            isMigrated: false,
            targetDisk: null,
            path: "~/exp",
            desc: "desc",
            tier: "experimental",
            running: false
          }
        ],
        loading: false,
        error: null
      }
    });

    expect(wrapper.text()).toContain("实验支持");
    expect(wrapper.text()).toContain("实验支持，迁移前需确认风险");
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
});
