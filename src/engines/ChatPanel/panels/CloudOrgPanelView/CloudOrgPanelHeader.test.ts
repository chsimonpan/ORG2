// @vitest-environment jsdom
import { Provider } from "jotai";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createSmokeRoot, dispatch } from "@src/test/reactSmokeHarness";

import CloudOrgPanelHeader from "./CloudOrgPanelHeader";
import { CLOUD_ORG_MANAGEMENT_TAB } from "./cloudOrgPanelTypes";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

const roots: Array<ReturnType<typeof createSmokeRoot>> = [];

afterEach(async () => {
  for (const root of roots.splice(0)) await root.unmount();
});

describe("CloudOrgPanelHeader", () => {
  it("shows the localized General, Sync, and Members tabs in order", () => {
    const markup = renderToStaticMarkup(
      createElement(
        Provider,
        null,
        createElement(CloudOrgPanelHeader, {
          orgId: "org-1",
          activeTab: CLOUD_ORG_MANAGEMENT_TAB.GENERAL,
          onTabChange: vi.fn(),
        })
      )
    );

    expect(markup).toContain('data-testid="cloud-org-tab-general"');
    expect(markup).toContain('data-testid="cloud-org-tab-sync"');
    expect(markup).toContain('data-testid="cloud-org-tab-members"');
    expect(markup).not.toContain('data-testid="cloud-org-tab-sessions"');
    expect(markup).not.toContain('data-testid="cloud-org-tab-repo-scope"');
    expect(markup.indexOf('data-testid="cloud-org-tab-general"')).toBeLessThan(
      markup.indexOf('data-testid="cloud-org-tab-sync"')
    );
    expect(markup.indexOf('data-testid="cloud-org-tab-sync"')).toBeLessThan(
      markup.indexOf('data-testid="cloud-org-tab-members"')
    );
    expect(markup).toContain("sections.general");
    expect(markup).toContain("cloud.orgPanel.sync.tabTitle");
    expect(markup).toContain("cloud.orgPanel.membersTitle");
  });

  it("reports the Sync tab key through onTabChange", async () => {
    const onTabChange = vi.fn();
    const root = createSmokeRoot();
    roots.push(root);
    await root.render(
      createElement(
        Provider,
        null,
        createElement(CloudOrgPanelHeader, {
          orgId: "org-1",
          activeTab: CLOUD_ORG_MANAGEMENT_TAB.GENERAL,
          onTabChange,
        })
      )
    );

    const syncTab = root.container.querySelector<HTMLButtonElement>(
      '[data-testid="cloud-org-tab-sync"]'
    );
    expect(syncTab).not.toBeNull();
    await dispatch(() => syncTab?.click());

    expect(onTabChange).toHaveBeenCalledWith(CLOUD_ORG_MANAGEMENT_TAB.SYNC);
  });
});
