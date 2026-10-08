// Run: npx tsx src/components/Settings/sections.test.ts

import {
  SECTION_ICONS,
  SETTINGS_SECTIONS,
  isSettingsLinkId,
  resolveSettingsLink,
  sectionTitle,
  type SectionId,
} from "./sections";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

test("guide section sits between logs and about", () => {
  const ids = SETTINGS_SECTIONS.map((s) => s.id);
  const logsIdx = ids.indexOf("logs");
  const guideIdx = ids.indexOf("guide");
  const aboutIdx = ids.indexOf("about");
  assert(logsIdx >= 0 && guideIdx >= 0 && aboutIdx >= 0, "missing nav ids");
  assert(guideIdx === logsIdx + 1, "guide should follow logs");
  assert(aboutIdx === guideIdx + 1, "about should follow guide");
});

test("jobs section sits right before logs", () => {
  const ids = SETTINGS_SECTIONS.map((s) => s.id);
  const jobsIdx = ids.indexOf("jobs");
  assert(jobsIdx >= 0, "missing jobs nav id");
  assert(ids[jobsIdx + 1] === "logs", "jobs should precede logs");
});

test("guide section title resolves", () => {
  assert(sectionTitle("guide" as SectionId) === "Guide", "guide title");
});

test("notification_settings_home: notifications sits between general and agents", () => {
  const ids = SETTINGS_SECTIONS.map((s) => s.id);
  const generalIdx = ids.indexOf("general");
  const notificationsIdx = ids.indexOf("notifications");
  const agentsIdx = ids.indexOf("agents");
  assert(generalIdx >= 0 && notificationsIdx >= 0 && agentsIdx >= 0, "missing nav ids");
  assert(notificationsIdx === generalIdx + 1, "notifications should follow general");
  assert(agentsIdx === notificationsIdx + 1, "agents should follow notifications");
});

test("notification_settings_home: notifications section title and icon resolve", () => {
  assert(sectionTitle("notifications" as SectionId) === "Notifications", "notifications title");
  assert(SECTION_ICONS.notifications, "notifications icon");
});

test("notification_settings_home: no nav id resolves to a different section", () => {
  for (const section of SETTINGS_SECTIONS) {
    const route = resolveSettingsLink(section.id);
    assert(route?.section === section.id && route.card === undefined, `${section.id} maps to itself`);
  }
  assert(resolveSettingsLink(undefined) === undefined, "no section stays no section");
});

test("the retired integrations link lands on the Messengers card in General", () => {
  const route = resolveSettingsLink("integrations");
  assert(route?.section === "general" && route.card === "telegram-bridge", "integrations routes to the Telegram bridge card");
  assert(!SETTINGS_SECTIONS.some((s) => (s.id as string) === "integrations"), "integrations is no longer a nav entry");
});

test("the messengers link opens the Messengers card and unknown links are rejected", () => {
  const route = resolveSettingsLink("messengers");
  assert(route?.section === "general" && route.card === "telegram-bridge", "messengers routes to the Telegram bridge card");
  assert(isSettingsLinkId("messengers") && isSettingsLinkId("general") && isSettingsLinkId("integrations"), "known link rejected");
  assert(!isSettingsLinkId("nope") && !isSettingsLinkId("toString"), "unknown link accepted");
});

test("notification_settings_home: the agents description no longer claims notifications", () => {
  const agents = SETTINGS_SECTIONS.find((s) => s.id === "agents");
  assert(agents, "agents entry");
  assert(!agents.desc.toLowerCase().includes("notification"), `stale copy: ${agents.desc}`);
});

test("every nav id has an icon and a unique label", () => {
  const labels = new Set<string>();
  for (const section of SETTINGS_SECTIONS) {
    assert(SECTION_ICONS[section.id], `${section.id} needs an icon`);
    assert(!labels.has(section.label), `duplicate nav label ${section.label}`);
    labels.add(section.label);
  }
});

let failed = 0;
for (const [name, fn] of tests) {
  try {
    fn();
    console.log(`  ✓ ${name}`);
  } catch (e) {
    failed++;
    console.error(`  ✗ ${name}: ${e instanceof Error ? e.message : e}`);
  }
}
console.log(`Settings/sections tests: ${failed === 0 ? "all passed" : `${failed} failed`}`);
process.exit(failed === 0 ? 0 : 1);
