export default function MetadataCredits({ credits }: { credits?: string[] }) {
  const application = credits === undefined
  const used = new Set(credits ?? ['tmdb', 'musicbrainz', 'coverartarchive', 'openlibrary'])
  if (!used.size) return null
  const links = [
    ['musicbrainz', 'MusicBrainz', 'https://musicbrainz.org/'],
    ['coverartarchive', 'Cover Art Archive', 'https://coverartarchive.org/'],
    ['openlibrary', 'Open Library', 'https://openlibrary.org/'],
    ['arr_sonarr', 'Sonarr', 'https://sonarr.tv/'],
    ['arr_radarr', 'Radarr', 'https://radarr.video/'],
    ['arr_lidarr', 'Lidarr', 'https://lidarr.audio/'],
  ]
  return <details className="metadata-credits"><summary>{application ? 'Application credits' : 'Metadata credits'}</summary><div>{application && <p>Credits for Grimoire’s metadata integrations. Item details identify the sources used for that item.</p>}{used.has('tmdb') && <><a href="https://www.themoviedb.org/" target="_blank" rel="noreferrer"><img src="/tmdb-logo.svg" width="100" height="13" alt="TMDB"/></a><p>This product uses the TMDB API but is not endorsed or certified by TMDB.</p></>}{links.filter(([id]) => used.has(id)).map(([id,label,url]) => <p key={id}>Metadata / artwork: <a href={url} target="_blank" rel="noreferrer">{label}</a>.</p>)}<p>Images remain the property of their respective rights holders.</p></div></details>
}
