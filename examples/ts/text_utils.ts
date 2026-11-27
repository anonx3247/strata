// Text helpers the Strata program in this folder calls (`extern ts`).

/** A post, as Strata's `Post` crosses: a plain object with its fields. */
interface Post {
  title: string;
  body: string;
  tags: string[];
}

/** A title as a URL slug: lowercase ASCII words joined by dashes. */
export function slugify(title: string): string {
  const ascii = title.normalize("NFKD").replace(/[^\x00-\x7f]/g, "");
  return ascii.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "");
}

/** How many times each word appears, by word. */
export function wordCounts(text: string): Record<string, number> {
  const counts: Record<string, number> = {};
  for (const word of text.toLowerCase().match(/[a-z']+/g) ?? []) {
    counts[word] = (counts[word] ?? 0) + 1;
  }
  return counts;
}

/** A post summed up as a Card. */
export function summary(post: Post) {
  const words = post.body.split(/\s+/).filter((w) => w !== "");
  return {
    type: "Card",
    slug: slugify(post.title),
    words: words.length,
    teaser: words.slice(0, 5).join(" "),
    tags: [...new Set(post.tags)].sort(),
  };
}

/** The year in `text`: throws a RangeError when there is none. */
export async function parseYear(text: string): Promise<number> {
  const match = text.match(/\b(1[0-9]{3}|20[0-9]{2})\b/);
  if (!match) throw new RangeError(`no year in '${text}'`);
  return Number(match[1]);
}
