# Images in chat

Ask your companion to create an image, then review the prompt and provider details in the image approval card. Choose **Allow once and generate** to send it. Image creation requires the `generate_image` permission, a saved OpenAI Platform API key, a configured workspace and durable conversation storage.

The first version uses `gpt-image-2.5-sunburst`, medium quality, one 1024 × 1024 PNG. It uses the canonical OpenAI Platform endpoint. Custom OpenAI-compatible endpoints and ChatGPT subscription credentials do not provide this image capability. Provider model access and organization verification may also be required.

Successful images appear in the conversation's Images gallery. Open a version to see it beside chat, use fullscreen, navigate versions or download the PNG. Each success creates a fresh attachment and immutable version record. The original is preserved. The workspace holds the bytes; changing or removing those files makes the affected saved preview unavailable rather than silently replacing its content.

## Edit a selected image

Open a saved version and choose **Edit in chat**, or attach a PNG, JPEG or WebP file normally. Describe the change and send the message. Attach exactly one image for an edit. Other file attachments do not select an image. The `edit_image` tool receives the exact image attached to the accepted user turn, rather than a path or URL supplied by the model.

Selecting a saved version reuses its original attachment identity. Sending it follows the ordinary chat attachment path and may transfer it to the chat model provider. A separate image approval card shows the edit instruction and the selected input before transferring them to OpenAI for generation. Only an exact owner **Allow once** decision approves that provider request. General network permission and remembered approval do not replace this review.

An edit creates another saved version. Editing an older version preserves the newer ones and records the chosen source as its parent. Selections stay scoped to their conversation; switching closes the preview, and clearing invalidates drafts from before the clear. Whole-image prompt edits are supported; brush masks and multiple reference images are outside this first slice.

## Charges and interrupted work

Image requests use the saved OpenAI API key and can incur API charges. Each physical request requires its own review. The adapter performs one request, with no automatic retry, fallback provider or key rotation. A local timeout or Stop cannot guarantee that remote processing or billing stopped.

The daemon saves dispatch intent before provider I/O. A lost response or an interrupted dispatch becomes an unknown outcome and is never automatically resent. Read the saved status before making a new request. Reopening the conversation only reads local status and versions.

Provider-reported image usage is retained with the operation when available. Image cost remains unknown in this slice; the text-token pricing table is not used to invent an image charge. Existing aggregate chat-cost totals do not include a known image price.

Stop, clear, deletion, ownership loss and changed conversation/project/workspace scope prevent a stale operation from publishing a new version. An interruption after remote generation but before acknowledged local publication can leave an unavailable output. Recovery does not regenerate it or claim a refund.

## Bounds and access

Prompts allow at most 2048 UTF-8 bytes. Input and output images allow at most 10 MiB. The reusable decoder also bounds source dimensions and decoded pixels. There are at most 32 image operations and 128 MiB of acknowledged image output per conversation. Capacity exhaustion preserves existing versions and asks for a new chat.

This release supports direct owner Web/API chats. Helpers, connectors, delegated/RPC execution and shared conversation views cannot authorize image requests. Image bytes remain behind the existing authenticated attachment route. Native operation/version provenance is separate from generic display results.

## Verification boundaries

Automated checks use fake image ports and loopback HTTP servers. They exercise request shape, byte limits, source identity, exact approval, immutable versions and interrupted outcomes. Local checks do not establish live OpenAI account/model availability, actual billing or output quality. A live-provider acceptance run requires separate authorization.

Provider behavior was checked against the official [image generation guide](https://developers.openai.com/api/docs/guides/image-generation), [image edits reference](https://developers.openai.com/api/reference/resources/images/methods/edit) and [pricing documentation](https://developers.openai.com/api/docs/pricing) on 30 October 2026.

## Backend compatibility and saved-byte observations

The gallery checks the responding daemon's advertised image-history contract. A regular chat remains quiet when an older backend does not advertise it. Known native image work receives compact compatibility or loading guidance. Refresh reads support and local history; it never resends generation. Failed refreshes preserve already known versions.

For each saved version, an optional host observation identifies the operation, attachment, immutable version, observation time and `saved_bytes_only` limit. `bytes_matched` includes the SHA-256 and byte count checked against the saved attachment record. Missing or changed bytes produce `unavailable`. A read budget or occupied physical reader produces `not_checked`. Historical request status stays separate from these fresh observations. An older backend without this evidence leaves saved bytes unobserved.

Readback is bounded to 16 MiB per batch and a fixed shared physical-reader capacity. A cancelled HTTP request cannot release physical capacity before its disk read finishes. Session clear, deletion, project rebinding, workspace change or changed ledger state invalidates stale response publication. Matching bytes do not establish visual correctness, user viewing or provider billing.
