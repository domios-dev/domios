/** Manifest capabilities.mediaDelivery, available only with a view. */
export interface MediaDeliveryDescriptor { types: MediaContentType[]; }
export type MediaContentType = "video/mp4" | "video/webm" | "video/ogg" | "audio/mpeg" | "audio/ogg" | "audio/wav" | "image/png" | "image/jpeg" | "image/webp" | "image/gif";
/** Backend-only mediaFile(id) result. The frame never supplies or receives this path. */
export interface MediaFile { path: string; contentType: MediaContentType; fileName?: string; }
/** ct.media(id) result in an iframe. Image handles are usable by gesture actions; playback URL is null. */
export interface PluginMediaHandle { handle: string; url: string | null; content_type: MediaContentType; file_name: string; expires_in_seconds: number; }
