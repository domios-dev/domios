/** What a channel add form submits; an empty name lets the channel's own index name it where the name is optional. */
export interface ChannelFormInput {
  name: string;
  url: string;
}

export function looksLikeChannelUrl(value: string): boolean {
  if (/^(git@|ssh:\/\/|https?:\/\/|file:\/\/)/.test(value)) return true;
  if (/^(\.?\.?\/|~\/|[A-Za-z]:[\\/])/.test(value)) return true;
  try {
    const parsed = new URL(value);
    return parsed.protocol.length > 1;
  } catch {
    return false;
  }
}

export type ChannelFormField = "name" | "url";

/** The first reason a channel add cannot be sent and the field it is about, or null when the input is ready. */
export function channelFormIssue(input: ChannelFormInput, { nameRequired }: { nameRequired: boolean }): { field: ChannelFormField; message: string } | null {
  const name = input.name.trim();
  const url = input.url.trim();
  if (nameRequired && (!name || !url)) return { field: name ? "url" : "name", message: "Channel name and URL are required." };
  if (!url) return { field: "url", message: "Channel URL is required." };
  if (name && !/^[A-Za-z0-9._-]+$/.test(name)) return { field: "name", message: "Channel name can use letters, numbers, dots, underscores, and hyphens." };
  if (!looksLikeChannelUrl(url)) return { field: "url", message: "Channel URL must be a URL, local path, or git SSH address." };
  return null;
}

/** The first reason a channel add cannot be sent, or null when the input is ready. */
export function channelFormError(input: ChannelFormInput, options: { nameRequired: boolean }): string | null {
  return channelFormIssue(input, options)?.message ?? null;
}
