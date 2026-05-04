#!/usr/bin/env python3
"""
Seed hwaiting.db from new_data.jsonl

This script:
1. Reads new_data.jsonl line by line
2. Generates SQL INSERT statements
3. Pipes them to sqlite3 CLI (which supports STRICT tables)
"""

import json
import subprocess
import sys
from pathlib import Path

def escape_sql_string(s):
    """Escape single quotes for SQL"""
    if s is None:
        return 'NULL'
    return "'" + str(s).replace("'", "''") + "'"

def extract_sentence_with_target(full_text, target):
    """
    Extract the first sentence containing the target word.
    
    Strategy:
    1. Split by newline (for dialogues)
    2. Find the part containing the target
    3. Split that part by period to get individual sentences
    4. Return the first sentence with the target
    
    Returns: (sentence, part_index, original_part) tuple
    """
    if not full_text or not target:
        return full_text, 0
    
    # Split by newline first (handles dialogue format)
    parts = [p.strip() for p in full_text.split('\n') if p.strip()]
    
    # Find which part contains the target word
    target_part = None
    part_index = 0
    for i, part in enumerate(parts):
        if target in part:
            target_part = part
            part_index = i
            break
    
    if not target_part:
        # Target not found, return first part
        first_part = parts[0] if parts else full_text
        return first_part, 0, first_part
    
    # Store original part before splitting
    original_part = target_part
    
    # Split by period to get individual sentences
    sentences = [s.strip() for s in target_part.split('.') if s.strip()]
    
    # Find first sentence with target
    for sentence in sentences:
        if target in sentence:
            # Add period back if it doesn't end with punctuation
            if not sentence[-1] in '.!?':
                sentence += '.'
            return sentence, part_index, original_part
    
    # Fallback: return the target part with period if needed
    if not target_part[-1] in '.!?':
        target_part += '.'
    return target_part, part_index, original_part

def extract_corresponding_translation(translation_text, part_index, korean_part, korean_full_text, original_korean_part):
    """
    Extract the sentence from translation that corresponds to the same dialogue part.
    
    Args:
        translation_text: Full translation text (may have multiple parts)
        part_index: Which dialogue part to extract (0-indexed)
        korean_part: The extracted Korean text to match sentence count
        korean_full_text: The original full Korean text to count preceding sentences
        original_korean_part: The untrimmed Korean part before sentence extraction
    """
    if not translation_text:
        return translation_text
    
    import re
    
    # Count sentences in the ORIGINAL Korean part (before sentence extraction)
    # This ensures we match the correct number of English sentences
    korean_sentence_count = len(re.findall(r'[.!?]', original_korean_part))
    if korean_sentence_count == 0:
        korean_sentence_count = 1
    
    # Split by newline (for dialogues)
    parts = [p.strip() for p in translation_text.split('\n') if p.strip()]
    
    # Check if translation has multiple parts by newline
    if len(parts) > 1 and part_index < len(parts):
        # Use the corresponding part (entire part, not just first sentence)
        target_part = parts[part_index]
        if target_part and not target_part[-1] in '.!?':
            target_part += '.'
        return target_part
    else:
        # Translation is a single paragraph - need to extract matching number of sentences
        # Split by sentence-ending punctuation (. ? !)
        sentences = re.split(r'(?<=[.!?])\s+', translation_text)
        sentences = [s.strip() for s in sentences if s.strip()]
        
        # Count how many sentences come before part_index in Korean
        korean_parts = [p.strip() for p in korean_full_text.split('\n') if p.strip()]
        sentences_before = 0
        for i in range(min(part_index, len(korean_parts))):
            sentences_before += len(re.findall(r'[.!?]', korean_parts[i]))
        
        # Extract the corresponding sentences from English
        start_idx = sentences_before
        end_idx = start_idx + korean_sentence_count
        
        if start_idx < len(sentences) and end_idx <= len(sentences):
            result_sentences = sentences[start_idx:end_idx]
            result = ' '.join(result_sentences)
            if result and not result[-1] in '.!?':
                result += '.'
            return result
        elif start_idx < len(sentences):
            # Take remaining sentences if we don't have enough
            result_sentences = sentences[start_idx:]
            result = ' '.join(result_sentences)
            if result and not result[-1] in '.!?':
                result += '.'
            return result
        else:
            # Fallback: return entire translation if indexing doesn't work
            return translation_text

def main():
    db_path = Path("hwaiting.db")
    data_path = Path("new_data.jsonl")
    
    # Check files exist
    if not data_path.exists():
        print(f"Error: {data_path} not found")
        sys.exit(1)
    
    if not db_path.exists():
        print(f"Error: {db_path} not found")
        print("Please create it first by running: sqlite3 hwaiting.db < new_schema.sql")
        sys.exit(1)
    
    # Generate SQL statements
    print(f"Processing {data_path}")
    sql_statements = ["BEGIN TRANSACTION;"]
    
    total_cards = 0
    total_translations = 0
    total_sentences = 0
    total_hints = 0
    
    with open(data_path) as f:
        for line_num, line in enumerate(f, 1):
            try:
                entry = json.loads(line)
                
                # Extract card data
                krdict_id = entry.get('target_code')  # Korean Dictionary ID
                word = entry['word']
                definition = entry['sense'][0]['definition']  # First sense only
                pos = entry['pos']
                origin_type = entry.get('original_origin')  # 고유어 / 한자어 / 외래어 / 혼종어
                hanja = entry.get('origin')  # Hanja characters (e.g., "價格")
                hanja_eum = entry.get('hanja_eum')  # Korean pronunciation of hanja
                grade = entry.get('word_grade')  # 초급 / 중급 / 고급
                frequency_rank = entry.get('frequency_rank')
                
                # Insert card (official cards don't have entries in custom_card_metadata)
                sql_statements.append(
                    f"INSERT INTO cards (krdict_id, word, definition, pos, origin_type, hanja, hanja_eum, grade, frequency_rank) "
                    f"VALUES ({krdict_id if krdict_id else 'NULL'}, {escape_sql_string(word)}, {escape_sql_string(definition)}, "
                    f"{escape_sql_string(pos)}, {escape_sql_string(origin_type)}, "
                    f"{escape_sql_string(hanja)}, {escape_sql_string(hanja_eum)}, "
                    f"{escape_sql_string(grade)}, {frequency_rank if frequency_rank else 'NULL'});"
                )
                card_id = line_num  # We'll use line number as card_id since they're inserted sequentially
                total_cards += 1
                
                # Insert card translation (first sense only)
                first_sense = entry['sense'][0]
                if first_sense.get('translation'):
                    trans = first_sense['translation'][0]  # First translation
                    trans_word = trans.get('trans_word')
                    trans_dfn = trans.get('trans_dfn')
                    
                    sql_statements.append(
                        f"INSERT INTO card_translations (card_id, language_tag, trans_word, trans_dfn) "
                        f"VALUES ({card_id}, 'en', {escape_sql_string(trans_word)}, {escape_sql_string(trans_dfn)});"
                    )
                    total_translations += 1
                
                # Insert sentence
                card_content = entry.get('card_content', {})
                korean_sentence = card_content.get('korean_sentence')
                target = card_content.get('target')
                
                if korean_sentence and target:
                    # Extract single sentence containing target word (returns tuple)
                    extracted_sentence, part_index, original_part = extract_sentence_with_target(korean_sentence, target)
                    
                    sql_statements.append(
                        f"INSERT INTO sentences (card_id, text, target) "
                        f"VALUES ({card_id}, {escape_sql_string(extracted_sentence)}, {escape_sql_string(target)});"
                    )
                    sentence_id = line_num  # Same as card_id since one sentence per card
                    total_sentences += 1
                    
                    # Insert sentence translation
                    sentence_translation = card_content.get('sentence_translation')
                    if sentence_translation:
                        # Extract corresponding sentence from translation using the same part_index and Korean text
                        extracted_translation = extract_corresponding_translation(sentence_translation, part_index, extracted_sentence, korean_sentence, original_part)
                        
                        sql_statements.append(
                            f"INSERT INTO sentence_translations (sentence_id, translation) "
                            f"VALUES ({sentence_id}, {escape_sql_string(extracted_translation)});"
                        )
                    
                    # Insert inflection hints (only for verbs/adjectives with conjugated forms)
                    speech_level = card_content.get('speech_level')
                    tense = card_content.get('tense')
                    
                    if speech_level and tense:
                        sql_statements.append(
                            f"INSERT INTO sentence_inflection_hints (sentence_id, speech_level, tense) "
                            f"VALUES ({sentence_id}, {escape_sql_string(speech_level)}, {escape_sql_string(tense)});"
                        )
                        total_hints += 1
                
                # Progress indicator
                if line_num % 100 == 0:
                    print(f"  Generated SQL for {line_num} entries...")
                    
            except Exception as e:
                print(f"Error on line {line_num} ({entry.get('word', '?')}): {e}")
                sys.exit(1)
    
    sql_statements.append("COMMIT;")
    
    # Execute SQL via sqlite3 CLI
    print(f"\nExecuting SQL statements...")
    sql_script = '\n'.join(sql_statements)
    
    try:
        result = subprocess.run(
            ['sqlite3', str(db_path)],
            input=sql_script,
            text=True,
            capture_output=True,
            check=True
        )
        
        if result.stderr:
            print(f"SQLite stderr: {result.stderr}")
        
    except subprocess.CalledProcessError as e:
        print(f"Error executing SQL: {e.stderr}")
        sys.exit(1)
    
    print(f"\nSeeding complete!")
    print(f"  Cards: {total_cards}")
    print(f"  Card translations: {total_translations}")
    print(f"  Sentences: {total_sentences}")
    print(f"  Sentence inflection hints: {total_hints}")
    print(f"\nDatabase saved to: {db_path}")

if __name__ == '__main__':
    main()