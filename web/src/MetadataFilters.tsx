import type { GroupBy, MediaItem } from './media'
export default function MetadataFilters({ items, genre, groupBy, onGenre, onGroup }: { items: MediaItem[]; genre: string; groupBy: GroupBy; onGenre: (genre: string) => void; onGroup: (group: GroupBy) => void }) {
  const genres = [...new Set(items.flatMap(item => item.metadata?.fields.genres ?? []))].sort()
  return <><label>Genre <select aria-label="Filter by genre" value={genre} onChange={event => onGenre(event.target.value)}><option value="">All genres</option>{genre && !genres.includes(genre) && <option value={genre}>{genre} (no current matches)</option>}{genres.map(genre => <option key={genre}>{genre}</option>)}</select></label><label>Group <select aria-label="Group media" value={groupBy} onChange={event => onGroup(event.target.value as GroupBy)}><option value="none">No grouping</option><option value="collection">Album / series</option><option value="creator">Artist / author / creator</option></select></label></>
}
