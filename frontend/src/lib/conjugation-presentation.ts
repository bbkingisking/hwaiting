import type { CardInflection } from './api'

// UI sections and wording are independent of the stored conjugation facts.
const sections: [string, [string, string][]][] = [
  ['Present', [['present_haeyo', 'Polite Informal'], ['present_hae', 'Intimate'], ['present_hasipsio', 'Formal'], ['present_haera', 'Plain']]],
  ['Past', [['past_haeyo', 'Polite Informal'], ['past_hae', 'Intimate'], ['past_hasipsio', 'Formal'], ['past_haera', 'Plain']]],
  ['Future', [['future_haeyo', 'Polite Informal'], ['future_hasipsio', 'Formal'], ['presumptive_haeyo', 'Presumptive · Polite Informal'], ['presumptive_hasipsio', 'Presumptive · Formal']]],
  ['Command & suggestion', [['request_haeyo', 'Request / honorific'], ['command_hasipsio', 'Command'], ['exclamation_haera', 'Exclamation'], ['propositive_haera', 'Propositive · Plain'], ['propositive_hasipsio', 'Propositive · Formal']]],
  ['Connectives', [['connective_and', 'And / so'], ['connective_but', 'But'], ['connective_if', 'If'], ['connective_cause', 'Because / and then'], ['connective_background', 'Background'], ['connective_intent', 'Intend to'], ['nominalizer', 'Nominalizer']]],
  ['Interrogative', [['question_hasipsio', 'Formal']]],
  ['Adnominal', [['adnominal_present_verb', 'Present · Verb'], ['adnominal_present_adj', 'Present · Adjective'], ['adnominal_past', 'Past'], ['adnominal_future', 'Future']]],
]

export function groupConjugations(inflections: CardInflection[]) {
  const remaining = new Map(inflections.map(form => [form.form_slug, form]))
  const groups = sections.map(([label, forms]) => ({ label, rows: forms.flatMap(([slug, name]) => {
    const form = remaining.get(slug)
    if (!form) return []
    remaining.delete(slug)
    return [{ ...form, label: name }]
  }) })).filter(group => group.rows.length > 0)
  // New backend definitions remain visible until the client adds presentation text.
  if (remaining.size) groups.push({ label: 'Other forms', rows: [...remaining.values()]
    .sort((a, b) => a.form_slug.localeCompare(b.form_slug))
    .map(form => ({ ...form, label: form.form_slug.replace(/_/g, ' ') })) })
  return groups
}
