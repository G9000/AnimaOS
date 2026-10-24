/** Legacy save payload; the fixed main character no longer uses these fields. */
export interface Traveler {
  name: string;
  skinTone: 'fair' | 'warm' | 'deep';
  hairStyle: 'short' | 'bob' | 'braid';
  hairColor: 'chestnut' | 'ink' | 'gold' | 'auburn';
  outfit: 'cloak' | 'tunic' | 'overalls';
  outfitColor: 'moss' | 'ochre' | 'berry';
}
