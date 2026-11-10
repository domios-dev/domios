import type {OptionalWireFields} from "./wireViews";
/** Manifest capabilities.mediaDelivery, available only with a view. */
export type MediaDeliveryDescriptor = import("./gen/MediaDeliveryDescriptor").MediaDeliveryDescriptor;
export type MediaContentType = import("./gen/MediaType").MediaType;
/** Backend-only mediaFile(id) result. The frame never supplies or receives this path. */
export type MediaFile = OptionalWireFields<import("./gen/MediaFile").MediaFile, "fileName">;
/** ct.media(id) result. Passive images passing the preview byte/dimension bounds share the exact-frame URL; unavailable previews return null. */
export type PluginMediaHandle = import("./gen/MediaHandle").MediaHandle;
