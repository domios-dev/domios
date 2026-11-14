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

/** The first reason a channel add cannot be sent, or null when the input is ready. */
export function channelFormError(input: ChannelFormInput, { nameRequired }: { nameRequired: boolean }): string | null {
  const name = input.name.trim();
  const url = input.url.trim();
  if (nameRequired && (!name || !url)) return "Channel name and URL are required.";
  if (!url) return "Channel URL is required.";
  if (name && !/^[A-Za-z0-9._-]+$/.test(name)) return "Channel name can use letters, numbers, dots, underscores, and hyphens.";
  if (!looksLikeChannelUrl(url)) return "Channel URL must be a URL, local path, or git SSH address.";
  return null;
}
