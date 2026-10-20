import { defineCollection } from 'astro:content';
import { docsLoader } from '@astrojs/starlight/loaders';
import { docsSchema } from '@astrojs/starlight/schema';
import type { CollectionConfig } from 'astro/content/config';

export const collections: Record<string, CollectionConfig<any>> = {
  docs: defineCollection({ loader: docsLoader(), schema: docsSchema() }),
};
