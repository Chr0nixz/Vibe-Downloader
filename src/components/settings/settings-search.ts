export type SettingsSearchSection<Id extends string = string> = {
  id: Id;
  title: string;
  description?: string;
  summary?: string;
  terms?: readonly string[];
};

export interface SettingsFieldKey {
  sectionId: string;
  fieldKey: string;
  terms?: readonly string[];
}

export interface SettingsFieldMatch {
  sectionId: string;
  fieldKey: string;
}

function normalizeSettingsQuery(query: string): string {
  return query.trim().toLocaleLowerCase();
}

export function settingsSectionMatchesQuery<Id extends string>(
  section: SettingsSearchSection<Id>,
  query: string,
): boolean {
  const normalizedQuery = normalizeSettingsQuery(query);
  if (!normalizedQuery) return true;
  return [section.id, section.title, section.description, section.summary, ...(section.terms ?? [])].some((value) =>
    value?.toLocaleLowerCase().includes(normalizedQuery),
  );
}

export function settingsSearchHasResults<Id extends string>(
  sections: readonly SettingsSearchSection<Id>[],
  query: string,
): boolean {
  const normalizedQuery = normalizeSettingsQuery(query);
  return !normalizedQuery || sections.some((section) => settingsSectionMatchesQuery(section, normalizedQuery));
}

export function findFieldMatches(
  fieldKeys: readonly SettingsFieldKey[],
  query: string,
): SettingsFieldMatch[] {
  const normalizedQuery = normalizeSettingsQuery(query);
  if (!normalizedQuery) return [];
  return fieldKeys
    .filter((field) =>
      [field.fieldKey, ...(field.terms ?? [])].some((value) =>
        value?.toLocaleLowerCase().includes(normalizedQuery),
      ),
    )
    .map((field) => ({ sectionId: field.sectionId, fieldKey: field.fieldKey }));
}
