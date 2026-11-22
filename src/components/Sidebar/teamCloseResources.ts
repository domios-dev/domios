export interface TeamCloseResource {
  id: string;
  name: string;
  members: Array<{ tabId: string; title: string; role: string | null; roleProfile: string | null }>;
  crons: string[];
  blueprints: string[];
  lanes: string[];
}
