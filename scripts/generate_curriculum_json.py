#!/usr/bin/env python3
"""Generate content/curriculum.json from the approved unit plan."""

from __future__ import annotations

import json
import re
from pathlib import Path

# slug, title, teaches, needs (comma-separated), source
P = tuple[str, str, str, str, str]

UNITS: list[dict] = [
  { "n": 1, "level": "N5", "title": "Saying what things are", "goal": "Introduce yourself and identify objects with です.", "points": [
    ["desu-da", "だ / です", "The copula: X is Y; polite です and plain だ", "", "keep"],
    ["wa-topic", "は (topic)", "Marks what the sentence is about: 私は学生です", "desu-da", "keep"],
    ["ka-question", "か and question words", "Yes/no questions; 何・だれ・どこ・いつ in the answer's slot", "wa-topic", "keep"],
    ["no-noun-link", "の (noun + noun)", "Possession, belonging, description: 私の本, 日本語の先生", "wa-topic", "keep"],
    ["mo-also", "も (also)", "Too / also; replaces は rather than stacking on it", "wa-topic", "keep"],
    ["ja-nai", "じゃないです / ではありません (and past)", "Negative and past of です, polite and plain", "desu-da", "keep"],
    ["kore-sore-are", "これ・それ・あれ・どれ", "Pointing words chosen by distance from speaker and listener", "wa-topic", "keep"],
    ["kono-sono-ano", "この・その・あの / ここ・そこ・あそこ", "Pointing + noun, and the matching place words", "kore-sore-are,no-noun-link", "keep"],
  ], "contrasts": [], "dojo": "です table: polite/plain × present/past × positive/negative", "reading": "自己紹介: a self-introduction, polite, about 80 characters" },
  { "n": 2, "level": "N5", "title": "What is where", "goal": "Say what exists, where it is, and how many.", "points": [
    ["ga-subject", "が (subject)", "Marks the subject when it is new or asked about: だれがいますか", "wa-topic", "new"],
    ["aru-iru", "ある / いる", "Existence and having; いる for people and animals", "ga-subject", "new"],
    ["ni-location", "に (where something is)", "Location of existence: 部屋に猫がいます", "aru-iru", "new"],
    ["position-nouns", "上・下・中・前・後ろ・隣", "Position words as nouns: 机の上に", "ni-location,no-noun-link", "new"],
    ["to-and", "と (and)", "Joins nouns into a complete list: パンと卵", "desu-da", "new"],
    ["ya-nado", "や / など", "Joins nouns into an open list: X, Y and so on", "to-and", "new"],
    ["numbers-counters", "Numbers and counters", "〜つ・〜人・〜本・〜枚・〜円, sound changes, counter after the particle: りんごが三つあります", "aru-iru", "new"],
  ], "contrasts": ["contrast-to-ya: と vs や (complete vs open list)"], "dojo": "Counter readings: いっぽん, さんぼん, ろっぽん", "reading": "My room: what is where" },
  { "n": 3, "level": "N5", "title": "Doing things", "goal": "Describe a day with ます verbs and the action particles.", "points": [
    ["masu-form", "Verb-ます", "ます・ません・ました・ませんでした; the three verb groups", "desu-da", "new"],
    ["wo-object", "を (object)", "What the action is done to: 本を読みます", "masu-form", "new"],
    ["de-place", "で (place of action)", "Where an action happens: 図書館で勉強します", "masu-form", "new"],
    ["ni-destination", "に (destination)", "Where you go, come or return to", "masu-form", "new"],
    ["he-direction", "へ (direction)", "Toward; overlaps with に for movement", "ni-destination", "new"],
    ["ni-time", "に (time) and telling time", "〜時・〜分・曜日に, ごろ; no に with 今日 or 毎日", "masu-form,numbers-counters", "new"],
    ["de-means", "で (means)", "Tool, transport, language: バスで, 日本語で", "de-place", "new"],
    ["to-with", "と (together with)", "Doing something with someone: 友達と", "to-and,masu-form", "new"],
  ], "contrasts": ["contrast-ni-de: に vs で for places (exists there vs happens there)", "contrast-ni-he: に vs へ (both fine for movement; where only に works)"], "dojo": "ます forms of every verb group, including する and 来る", "reading": "My weekend: a polite diary entry" },
  { "n": 4, "level": "N5", "title": "Describing and asking", "goal": "Describe things, say what you like, order and invite.", "points": [
    ["i-adjective", "い-adjectives", "Present, negative, past, before nouns; いい→よくない", "desu-da", "new"],
    ["na-adjective", "な-adjectives", "な before nouns; じゃない, でした", "i-adjective,ja-nai", "new"],
    ["suki-jouzu", "が with 好き・上手・分かる", "The thing liked, good at or understood takes が", "ga-subject,na-adjective", "new"],
    ["amari-zenzen", "あまり / 全然 + negative", "Degree words that need a negative; とても, ちょっと", "i-adjective", "new"],
    ["yo-ne", "よ / ね", "よ for news to the listener, ね for shared ground", "desu-da", "new"],
    ["wo-kudasai", "N をください", "Asking for things in shops and restaurants", "wo-object,numbers-counters", "new"],
    ["kara-made", "から / まで (from / until)", "Start and end points in time and space", "ni-time", "new"],
    ["masenka-mashou", "〜ませんか / 〜ましょう(か)", "Inviting, suggesting and offering", "masu-form", "new"],
    ["demo-suggestion", "でも (suggestion)", "Suggestion でも for offering an example option: お茶でも飲みませんか", "masenka-mashou", "new"],
  ], "contrasts": ["contrast-wa-ga-1: は vs が, first pass (topic vs new information)"], "dojo": "い- and な-adjective forms", "reading": "At a café: a short ordering dialogue" },
  { "n": 5, "level": "N5", "title": "Wanting and comparing", "goal": "Say what you want, why, and which is better.", "points": [
    ["hoshii", "〜がほしい", "Wanting a thing", "suki-jouzu", "new"],
    ["tai", "〜たい", "Wanting to do", "masu-form,hoshii", "keep"],
    ["ni-iku", "Verb stem + に行く", "Going somewhere to do something", "masu-form,ni-destination", "new"],
    ["kara-reason", "〜から (reason)", "Giving a reason; polite clauses now, plain ones in Unit 7", "masu-form,i-adjective", "keep"],
    ["naru", "〜になる / 〜くなる", "Becoming: 高くなる, 医者になる", "i-adjective,na-adjective", "new"],
    ["yori-hou-ga", "より / ほうが / どちら", "Comparing two things", "i-adjective", "new"],
    ["ichiban", "〜の中で〜が一番", "Superlatives", "yori-hou-ga", "new"],
  ], "contrasts": [], "dojo": "", "reading": "Planning a trip: a polite message to a friend" },
  { "n": 6, "level": "N5", "title": "The て-form", "goal": "Link actions, make requests, give and refuse permission.", "points": [
    ["te-form", "Verb-て (and then)", "Linking actions in order; the て-form sound changes", "masu-form", "new"],
    ["soshite-sorekara", "そして / それから / でも", "Sentence connectors for sequence and contrast", "te-form", "new"],
    ["te-kudasai", "〜てください", "Asking someone to do something", "te-form", "new"],
    ["te-iru", "〜ている", "Ongoing action and resulting state", "te-form", "keep"],
    ["te-mo-ii", "〜てもいい", "Giving and asking permission", "te-form", "keep"],
    ["te-wa-ikenai", "〜てはいけない", "Prohibition", "te-mo-ii", "keep"],
    ["te-kara", "〜てから", "After doing X, then Y", "te-form", "new"],
    ["adj-te", "〜くて / 〜で", "Linking adjectives and nouns: 安くておいしい", "te-form,na-adjective", "new"],
  ], "contrasts": [], "dojo": "て-form across every godan ending, plus 行く→行って", "reading": "Dormitory rules: a notice with てください and てはいけない" },
  { "n": 7, "level": "N5", "title": "Plain forms and casual speech", "goal": "Talk to friends and build clauses that modify nouns.", "points": [
    ["plain-nonpast", "Dictionary form and ない-form", "Plain present, positive and negative", "masu-form", "new"],
    ["plain-past", "た / なかった", "Plain past, positive and negative", "plain-nonpast,te-form", "new"],
    ["casual-speech", "Casual speech", "Dropping です/ます; だ, よ and rising questions between friends", "plain-past,yo-ne", "new"],
    ["to-omou", "〜と思う", "Saying what you think", "plain-past", "keep"],
    ["to-itte-ita", "〜と言っていました", "Reporting what someone said", "to-omou", "new"],
    ["naide-kudasai", "〜ないでください", "Asking someone not to do something", "plain-nonpast,te-kudasai", "new"],
    ["noun-modifier", "Clause + noun", "Relative clauses: 昨日買った本, 眼鏡をかけている人", "plain-past", "new"],
    ["n-desu", "〜んです", "Explaining, and asking for an explanation", "plain-past", "new"],
  ], "contrasts": [], "dojo": "ない and た forms; casual ↔ polite conversion", "reading": "A LINE chat between two friends (casual)" },
  { "n": 8, "level": "N5", "title": "Time and experience", "goal": "Sequence events and talk about experiences.", "points": [
    ["mae-ni", "〜前に", "Before doing X", "plain-nonpast", "keep"],
    ["ato-de", "〜あとで", "After doing X", "plain-past", "keep"],
    ["toki", "〜とき", "When X; how tense inside とき works", "plain-past,noun-modifier", "new"],
    ["nagara", "〜ながら", "Doing two things at once", "masu-form,te-iru", "keep"],
    ["tari-tari", "〜たり〜たりする", "Listing example actions", "plain-past", "new"],
    ["ta-koto-ga-aru", "〜たことがある", "Having done something before", "plain-past", "keep"],
    ["mou-mada", "もう〜た / まだ〜ていない", "Already done, not yet done", "te-iru,plain-past", "new"],
  ], "contrasts": ["contrast-mae-ato-tekara: 前に vs あとで vs てから"], "dojo": "", "reading": "A travel diary" },
  { "n": 9, "level": "N5", "title": "Ability, advice and obligation", "goal": "Say what you can, should, must and needn't do.", "points": [
    ["koto-ga-dekiru", "〜ことができる", "Ability and possibility", "plain-nonpast", "keep"],
    ["no-ga-jouzu", "〜のが上手 / 好き", "Turning a verb into a noun to like it or be good at it", "suki-jouzu,plain-nonpast", "keep"],
    ["hou-ga-ii", "〜ほうがいい", "Advice", "plain-past", "keep"],
    ["sugiru", "〜すぎる", "Too much", "i-adjective,masu-form", "new"],
    ["nakereba-naranai", "〜なければならない", "Obligation; also なきゃ and ないといけない", "plain-nonpast", "keep"],
    ["nakute-mo-ii", "〜なくてもいい", "No need to", "nakereba-naranai,te-mo-ii", "keep"],
    ["deshou", "でしょう / だろう", "Probably; checking with the listener", "plain-past", "new"],
  ], "contrasts": ["contrast-permission-obligation: てもいい / てはいけない / なければならない / なくてもいい as one grid"], "dojo": "", "reading": "Advice from a clinic: a short notice" },
  { "n": 10, "level": "N5", "title": "Plans, reasons and linking", "goal": "Wrap up N5: intentions, soft reasons, but-clauses.", "points": [
    ["tsumori", "〜つもり", "Intention", "plain-nonpast", "keep"],
    ["node", "〜ので", "Softer, more explanatory reason", "kara-reason,plain-past", "keep"],
    ["kedo-ga", "〜けど / 〜が (but)", "Linking contrasting clauses; softening a request", "plain-past", "new"],
    ["nanika-nanimo", "何か / 何も and family", "Something / nothing, somewhere / nowhere, someone / no one", "ka-question,plain-nonpast", "new"],
  ], "contrasts": ["contrast-kara-node: から vs ので (tone and politeness, not right vs wrong)", "contrast-wa-ga-2: は vs が, second pass (contrast, exhaustive が, は in negatives)"], "dojo": "Mixed review of every N5 form", "reading": "An email to a teacher" },
  { "n": 11, "level": "N4", "title": "Can, looks like, try", "goal": "Ability with the potential form, appearances, limits.", "points": [
    ["potential", "Potential form", "Can do: 読める, 食べられる; が or を with it", "plain-nonpast,koto-ga-dekiru", "new"],
    ["you-ni-naru", "〜ようになる", "Coming to be able to; gradual change", "potential", "keep"],
    ["sou-appearance", "〜そう (looks like)", "おいしそう, 雨が降りそう: judging from appearance", "i-adjective,masu-form", "new"],
    ["te-miru", "〜てみる", "Try doing and see", "te-form", "keep"],
    ["shi-listing", "〜し、〜し", "Listing reasons", "plain-past", "keep:N5→N4"],
    ["toka-toka", "〜とか〜とか", "Casual example listing", "ya-nado,casual-speech", "new"],
    ["dake", "だけ", "Only, just", "numbers-counters", "new"],
    ["kai-mo", "〜に〜回 / 〜も", "Frequency (〜回) and emphatic も (as many as)", "dake", "new"],
    ["shika-negative", "〜しか〜ない", "Nothing but / only, with a negative", "dake", "new"],
  ], "contrasts": ["contrast-shika-dake: しか〜ない vs だけ"], "dojo": "Potential form; ら-dropped 見れる flagged as casual", "reading": "What I can and can't do at my new job" },
  { "n": 12, "level": "N4", "title": "Giving, receiving and favors", "goal": "Get the viewpoint of giving verbs right.", "points": [
    ["ni-recipient", "に (person: to / from)", "に marks the person receiving with あげる/教える/貸す and the source with もらう/借りる/習う, and から can replace に for the source: 友達に本をあげます / 友達に（から）本をもらいました.", "ni-destination,masu-form", "new"],
    ["ageru-kureru-morau", "あげる / くれる / もらう", "Who gives to whom, from whose viewpoint; person に (recipient) and に/から (giver with もらう)", "ni-recipient,ga-subject", "new"],
    ["te-ageru-morau", "〜てあげる / 〜てもらう", "Doing and receiving favors", "ageru-kureru-morau,te-form", "new"],
    ["te-kureru", "〜てくれる", "Someone does something for me", "te-ageru-morau", "keep"],
    ["te-hoshii", "〜てほしい", "Wanting someone else to do something", "te-kureru,hoshii", "new"],
    ["itadakemasenka", "〜ていただけませんか / 〜てくれない？", "Polite and casual requests", "te-kureru", "new"],
    ["te-yokatta", "〜てよかった / 〜てくれてありがとう", "Relief, and thanks for an action", "te-kureru", "new"],
  ], "contrasts": ["contrast-giving: あげる vs くれる vs もらう (viewpoint)"], "dojo": "", "reading": "Thank-you note after a homestay" },
  { "n": 13, "level": "N4", "title": "Intentions and decisions", "goal": "Plans, decisions, and questions inside sentences.", "points": [
    ["volitional", "Volitional 〜よう / 〜おう", "Let's, in plain speech", "plain-nonpast", "new"],
    ["you-to-omou", "〜(よ)うと思う", "Thinking of doing", "volitional,to-omou", "new"],
    ["te-oku", "〜ておく", "Doing in advance; leaving as is", "te-form", "keep"],
    ["koto-ni-suru", "〜ことにする", "Deciding to do", "plain-nonpast", "keep"],
    ["koto-ni-naru", "〜ことになる", "It has been decided; it turns out", "koto-ni-suru", "keep:N3→N4"],
    ["yotei", "〜予定だ", "Scheduled plans", "noun-modifier", "new"],
    ["kadouka", "〜か / 〜かどうか", "Questions inside sentences: 何時か分からない", "ka-question,plain-past", "new"],
  ], "contrasts": ["contrast-intention: つもり vs (よ)うと思う vs 予定", "contrast-koto-ni-suru-naru: ことにする vs ことになる"], "dojo": "Volitional form", "reading": "An email about weekend plans" },
  { "n": 14, "level": "N4", "title": "Conditions", "goal": "The four conditionals, regret, hope and even-if.", "points": [
    ["to-conditional", "〜と (whenever)", "Automatic results; directions", "plain-nonpast", "new"],
    ["tara-conditional", "〜たら", "If / when; the widest conditional", "plain-past", "keep"],
    ["ba-conditional", "〜ば", "If (hypothetical, general condition)", "plain-nonpast", "keep"],
    ["ba-ii", "〜ばいい / どうしたらいいですか", "Advice with ばいい, and asking what one should do: どうしたらいいですか", "ba-conditional,tara-conditional", "new"],
    ["tara-dou", "〜たらどうですか", "Suggestions", "tara-conditional", "new"],
    ["nara-conditional", "〜なら", "If that's the case; speaking of", "plain-past", "keep"],
    ["ba-yokatta", "〜ばよかった", "Regret: I should have", "ba-conditional", "new"],
    ["to-ii", "〜といい(ですね)", "Hoping", "to-conditional", "new"],
    ["te-mo-even-if", "〜ても", "Even if", "te-form", "new"],
    ["baai-wa", "〜場合（は）", "In the case that: 〜場合は / 〜場合に", "plain-past,no-noun-link", "new"],
  ], "contrasts": ["contrast-conditionals: と / ば / たら / なら"], "dojo": "ば-form and たら-form", "reading": "Directions to a friend's house" },
  { "n": 15, "level": "N4", "title": "Guessing, hearsay and feelings", "goal": "Mark how sure you are and where you heard it.", "points": [
    ["kamo-shirenai", "〜かもしれない", "Might", "plain-past", "keep"],
    ["sou-da-hearsay", "〜そうだ (hearsay)", "I heard that", "plain-past", "keep"],
    ["you-da", "〜ようだ", "It seems, from evidence", "plain-past,noun-modifier", "keep"],
    ["mitai", "〜みたい", "Seems; like (casual)", "you-da,casual-speech", "new"],
    ["rashii", "〜らしい", "Apparently; typical of", "sou-da-hearsay", "new"],
    ["hazu-da", "〜はずだ", "Should be (logical expectation)", "plain-past", "keep"],
    ["hazu-ga-nai", "〜はずがない", "Can't possibly be", "hazu-da", "new"],
    ["tte", "〜って", "Casual quoting and topic marking", "to-itte-ita,casual-speech", "new"],
    ["garu", "〜がる / 〜たがる", "Other people's feelings and wishes", "tai,i-adjective", "new"],
  ], "contrasts": ["contrast-evidential: そう (looks) / そうだ (heard) / ようだ / らしい / みたい", "contrast-certainty: はずだ vs でしょう vs かもしれない"], "dojo": "", "reading": "Office gossip: a casual dialogue" },
  { "n": 16, "level": "N4", "title": "Completion, states and change", "goal": "Finished actions, states left behind, change over time.", "points": [
    ["te-shimau", "〜てしまう", "Finish completely; regret; ちゃう", "te-form", "keep"],
    ["jidoushi-tadoushi", "Transitive / intransitive pairs", "開ける・開く, 消す・消える: who did it vs what happened", "wo-object,ga-subject", "new"],
    ["te-aru", "〜てある", "A state someone left on purpose", "jidoushi-tadoushi,te-iru", "new"],
    ["tokoro-da", "〜ところだ", "About to, in the middle of, just did", "plain-past,te-iru", "new"],
    ["ta-bakari", "〜たばかり", "Just did, recently", "plain-past", "split:bakari"],
    ["te-kuru-iku", "〜てくる / 〜ていく", "Change toward now or onward; go and come back", "te-form", "new"],
    ["mama", "〜まま", "Leaving a state unchanged", "noun-modifier", "new"],
    ["naide-without", "〜ないで (without doing)", "Doing Y without doing X", "plain-nonpast", "new"],
    ["fukugou-doushi", "〜始める / 〜終わる / 〜続ける / 〜出す", "Start, finish, keep doing", "masu-form", "new"],
  ], "contrasts": ["contrast-te-iru-te-aru: 開いている vs 開けてある", "contrast-just-did: たところ vs たばかり"], "dojo": "Transitive/intransitive pair drill", "reading": "Moving into a new apartment" },
  { "n": 17, "level": "N4", "title": "Purpose, effort and concession", "goal": "Goals, deadlines, how-to, and even-though.", "points": [
    ["tame-ni", "〜ために", "In order to (purpose)", "plain-nonpast", "keep"],
    ["you-ni-purpose", "〜ように (so that)", "Purpose with non-volitional and potential verbs", "potential", "new"],
    ["you-ni-suru", "〜ようにする", "Making an effort to", "you-ni-purpose", "keep"],
    ["made-ni", "〜までに", "By (a deadline)", "kara-made", "new"],
    ["yasui-nikui", "〜やすい / 〜にくい", "Easy / hard to do", "masu-form", "keep"],
    ["kata", "〜方", "How to do", "masu-form", "new"],
    ["noni", "〜のに", "Even though (unexpected)", "plain-past", "keep"],
    ["no-you-na", "〜のような / 〜のように", "Like N: similes and examples", "you-da", "new"],
    ["koto-ga-aru-sometimes", "〜ことがある (sometimes)", "There are times when", "ta-koto-ga-aru", "keep:N3→N4"],
  ], "contrasts": ["contrast-tame-youni: ために vs ように", "contrast-made-madeni: まで vs までに"], "dojo": "", "reading": "A study-tips column" },
  { "n": 18, "level": "N4", "title": "Voice and politeness", "goal": "Passive, causative and the basics of keigo.", "points": [
    ["passive", "Passive 〜られる", "Direct passive and the suffering passive", "plain-nonpast,jidoushi-tadoushi", "new"],
    ["causative", "Causative 〜させる", "Make or let someone do", "plain-nonpast", "new"],
    ["causative-passive", "Causative-passive 〜させられる", "Being made to do", "causative,passive", "new"],
    ["sasete-kudasai", "〜させてください", "Let me do", "causative,te-kudasai", "new"],
    ["meireikei", "Imperative and 〜な", "Commands, signs, sports", "plain-nonpast", "new"],
    ["nasai", "〜なさい", "Instructions from parents and teachers", "masu-form", "new"],
    ["sonkeigo", "Honorific speech (尊敬語)", "いらっしゃる, お〜になる: raising the other person", "masu-form,passive", "new"],
    ["kenjougo", "Humble speech (謙譲語)", "参る, お〜する: lowering yourself", "sonkeigo", "new"],
  ], "contrasts": ["contrast-rareru: られる as potential, passive or honorific", "contrast-voice: passive vs causative vs causative-passive"], "dojo": "Passive, causative, causative-passive, imperative, keigo verb swaps", "reading": "A shop announcement and a polite business email" },
  { "n": 19, "level": "N3", "title": "Cause, result and attitude", "goal": "Reasons that carry the speaker's attitude.", "points": [
    ["okage-de", "〜おかげで", "Thanks to (good result)", "plain-past", "keep"],
    ["sei-de", "〜せいで", "Because of (blame)", "plain-past", "keep"],
    ["tame-ni-cause", "〜ために (because of)", "Formal cause; a separate sense from purpose", "tame-ni,plain-past", "new"],
    ["wake-da", "〜わけだ", "That's why; so it follows", "n-desu", "new"],
    ["wake-dewa-nai", "〜わけではない", "It doesn't mean that", "wake-da", "keep"],
    ["wake-ga-nai", "〜わけがない", "There's no way that", "wake-da", "new"],
    ["kara-koso", "〜からこそ", "Precisely because", "kara-reason", "new"],
  ], "contrasts": ["contrast-okage-sei: おかげで vs せいで vs ために"], "dojo": "", "reading": "A short opinion column" },
  { "n": 20, "level": "N3", "title": "Talking about a topic", "goal": "Relation phrases for news, essays and work talk.", "points": [
    ["ni-tsuite", "〜について", "About", "no-noun-link", "keep"],
    ["ni-kanshite", "〜に関して", "Concerning (more formal)", "ni-tsuite", "new"],
    ["ni-taishite", "〜に対して", "Toward, against, in contrast to", "ni-tsuite", "keep"],
    ["ni-totte", "〜にとって", "For, from the standpoint of", "ni-tsuite", "keep"],
    ["to-shite", "〜として", "As (in the role of)", "desu-da", "new"],
    ["ni-yotte", "〜によって", "Depending on, by, due to (senses taught separately)", "passive", "keep"],
    ["ni-yoru-to", "〜によると", "According to", "sou-da-hearsay", "new"],
    ["ni-kurabete", "〜に比べて", "Compared to", "yori-hou-ga", "keep"],
  ], "contrasts": ["contrast-topic-phrases: について vs に対して vs にとって"], "dojo": "", "reading": "A news-style paragraph about a survey" },
  { "n": 21, "level": "N3", "title": "Time, sequence and change", "goal": "Timing and gradual change in narratives.", "points": [
    ["uchi-ni", "〜うちに", "While (before things change)", "plain-nonpast", "keep"],
    ["ta-totan", "〜たとたん", "The moment that", "plain-past", "new"],
    ["tsuide-ni", "〜ついでに", "While you're at it", "noun-modifier", "new"],
    ["tokoro-e", "〜ところに / 〜ところへ", "Just as something happened", "tokoro-da", "new"],
    ["ni-tsurete", "〜につれて", "As X changes, Y changes", "naru", "keep:N2→N3"],
    ["ippou-da", "〜一方だ", "Keeps getting more and more", "naru", "keep:N2→N3"],
    ["hodo", "〜ほど / 〜くらい", "Extent and rough amounts", "yori-hou-ga", "new"],
    ["ba-hodo", "〜ば〜ほど", "The more…, the more…", "ba-conditional,hodo", "new"],
  ], "contrasts": ["contrast-change: につれて vs 一方だ vs ようになる"], "dojo": "", "reading": "How my town has changed" },
  { "n": 22, "level": "N3", "title": "Concession, alternatives and assumption", "goal": "Even-though, even-if and instead.", "points": [
    ["to-shitara", "〜としたら", "Assuming that", "tara-conditional", "keep:N2→N3"],
    ["to-shite-mo", "〜としても", "Even supposing", "te-mo-even-if,to-shitara", "new"],
    ["ni-shite-wa", "〜にしては", "For (considering that)", "plain-past", "new"],
    ["kuse-ni", "〜くせに", "Even though (critical)", "noni", "new"],
    ["nagara-mo", "〜ながらも", "Although (concessive ながら)", "nagara", "new"],
    ["kawari-ni", "〜かわりに", "Instead of; in exchange for", "noun-modifier", "keep"],
  ], "contrasts": ["contrast-concession: のに vs くせに vs ながらも vs にしては"], "dojo": "", "reading": "A product review" },
  { "n": 23, "level": "N3", "title": "Judgment and necessity", "goal": "Should, must, can't, no need.", "points": [
    ["beki-da", "〜べきだ", "Should (duty or logic)", "plain-nonpast", "keep"],
    ["koto-wa-nai", "〜ことはない", "There's no need to", "plain-nonpast", "keep:N2→N3"],
    ["wake-niwa-ikanai", "〜わけにはいかない", "Can't very well", "wake-da", "keep:N2→N3"],
    ["ni-chigainai", "〜に違いない", "Must be (strong guess)", "hazu-da", "keep:N2→N3"],
    ["shika-nai", "〜しかない", "Have no choice but", "shika-negative", "new"],
    ["koto-da", "〜ことだ (advice)", "The best thing is to", "plain-nonpast,hou-ga-ii,beki-da", "new"],
    ["mono-da", "〜ものだ", "That's how things are; used to; should", "plain-past", "new"],
  ], "contrasts": ["contrast-should: べきだ vs ほうがいい vs ことだ", "contrast-wake: わけだ / わけではない / わけがない / わけにはいかない"], "dojo": "", "reading": "An advice-column reply" },
  { "n": 24, "level": "N3", "title": "Quoting, naming and explaining", "goal": "Define, report and pass on requests.", "points": [
    ["to-iu-noun", "〜という N", "Called; the fact that", "to-itte-ita", "new"],
    ["to-iu-koto-da", "〜ということだ", "It means that; I heard that", "to-iu-noun", "keep"],
    ["to-iu-no-wa", "〜というのは", "Defining a word or idea", "to-iu-noun", "new"],
    ["you-ni-iu", "〜ように言う / 頼む", "Reported requests and orders", "you-ni-purpose", "new"],
  ], "contrasts": [], "dojo": "", "reading": "A blog post explaining a custom" },
  { "n": 25, "level": "N3", "title": "Degree, extent and emphasis", "goal": "How much, how often, and even.", "points": [
    ["bakari", "〜ばかり (nothing but)", "Only / nothing but; たばかり moved to its own point", "dake", "keep"],
    ["gachi", "〜がち", "Tend to (usually bad)", "masu-form", "new"],
    ["ppoi", "〜っぽい", "-ish; seems like", "casual-speech", "new"],
    ["dake-de-naku", "〜だけでなく", "Not only … but also", "dake", "new"],
    ["sae", "〜さえ", "Even", "mo-also", "new"],
    ["ue-ni", "〜うえに", "On top of that", "noun-modifier", "new"],
    ["ppanashi", "〜っぱなし", "Left on; left undone", "mama", "new"],
  ], "contrasts": ["contrast-bakari-dake-shika: ばかり vs だけ vs しか"], "dojo": "", "reading": "Complaints about a roommate (casual)" },
]

# Explicit contrast member overrides when inference from unit points is ambiguous.
CONTRAST_MEMBERS: dict[str, list[str]] = {
    "contrast-to-ya": ["to-and", "ya-nado"],
    "contrast-ni-de": ["ni-location", "de-place"],
    "contrast-ni-he": ["ni-destination", "he-direction"],
    "contrast-wa-ga-1": ["wa-topic", "ga-subject"],
    "contrast-mae-ato-tekara": ["mae-ni", "ato-de", "te-kara"],
    "contrast-permission-obligation": ["te-mo-ii", "te-wa-ikenai", "nakereba-naranai", "nakute-mo-ii"],
    "contrast-kara-node": ["kara-reason", "node"],
    "contrast-wa-ga-2": ["wa-topic", "ga-subject"],
    "contrast-shika-dake": ["shika-negative", "dake"],
    "contrast-giving": ["ageru-kureru-morau", "te-ageru-morau", "te-kureru"],
    "contrast-intention": ["tsumori", "you-to-omou", "yotei"],
    "contrast-koto-ni-suru-naru": ["koto-ni-suru", "koto-ni-naru"],
    "contrast-conditionals": ["to-conditional", "ba-conditional", "tara-conditional", "nara-conditional"],
    "contrast-evidential": ["sou-appearance", "sou-da-hearsay", "you-da", "rashii", "mitai"],
    "contrast-certainty": ["hazu-da", "deshou", "kamo-shirenai"],
    "contrast-te-iru-te-aru": ["te-iru", "te-aru"],
    "contrast-just-did": ["tokoro-da", "ta-bakari"],
    "contrast-tame-youni": ["tame-ni", "you-ni-purpose"],
    "contrast-made-madeni": ["kara-made", "made-ni"],
    "contrast-rareru": ["potential", "passive", "sonkeigo"],
    "contrast-voice": ["passive", "causative", "causative-passive"],
    "contrast-okage-sei": ["okage-de", "sei-de", "tame-ni-cause"],
    "contrast-topic-phrases": ["ni-tsuite", "ni-taishite", "ni-totte"],
    "contrast-change": ["ni-tsurete", "ippou-da", "you-ni-naru"],
    "contrast-concession": ["noni", "kuse-ni", "nagara-mo", "ni-shite-wa"],
    "contrast-should": ["beki-da", "hou-ga-ii", "koto-da"],
    "contrast-wake": ["wake-da", "wake-dewa-nai", "wake-ga-nai", "wake-niwa-ikanai"],
    "contrast-bakari-dake-shika": ["bakari", "dake", "shika-negative"],
}

# Pedagogical summaries for contrast points (title stays the short label).
# Style: one plain-English rule + Japanese minimal pair(s); polite です/ます
# unless the contrast is about casual speech; standard kanji, no furigana.
CONTRAST_SUMMARIES: dict[str, str] = {
    "contrast-to-ya": "と lists a complete set; や leaves the list open: パンと卵を買いました / パンや卵を買いました。",
    "contrast-ni-de": "に marks where something exists; で marks where an action happens: 部屋に猫がいます / 部屋で勉強します.",
    "contrast-ni-he": "に and へ both mark destination for movement; only に works for time (and existence/purpose): 学校に行きます / 学校へ行きます / 七時に起きます（×七時へ）。",
    "contrast-wa-ga-1": "は marks the topic; が marks what is new or asked about: だれが来ましたか。—田中さんが来ました / 田中さんは来ましたか。",
    "contrast-mae-ato-tekara": "前に means before doing; あとで means after doing; てから means after X, then Y: 行く前に電話します / 行ったあとで電話します / 行ってから電話します。",
    "contrast-permission-obligation": "てもいい allows; てはいけない forbids; なければならない obliges; なくてもいい means no need: 食べてもいいです / 食べてはいけません / 食べなければなりません / 食べなくてもいいです。",
    "contrast-kara-node": "から states a reason more assertively; ので is softer and more explanatory: 雨が降ったから行きませんでした / 雨が降ったので行きませんでした。",
    "contrast-wa-ga-2": "は marks contrastive focus; が marks the subject inside a subordinate clause: 魚は食べますが、肉は食べません / 雨が降ったとき、家にいました。",
    "contrast-shika-dake": "だけ means only (neutral); しか〜ない means nothing but and needs a negative: 水だけ飲みます / 水しか飲みません。",
    "contrast-giving": "あげる gives outward from my side; くれる gives toward me; もらう means I receive: 私は友達に本をあげます / 友達が私に本をくれます / 私は友達に本をもらいます。",
    "contrast-intention": "つもり is intention; (よ)うと思う is thinking of doing; 予定 is a scheduled plan: 日本へ行くつもりです / 日本へ行こうと思います / 日本へ行く予定です。",
    "contrast-koto-ni-suru-naru": "ことにする is a decision I make; ことになる is decided for me or turns out that way: 毎日走ることにしました / 毎日走ることになりました。",
    "contrast-conditionals": "と is an automatic result; ば is hypothetical; たら is widest if/when; なら is if that's the case: 春になると暖かくなります / 安ければ買います / 雨が降ったら行きません / 行くなら早く行ってください。",
    "contrast-evidential": "そう is looks like; そうだ is hearsay; ようだ is from evidence; らしい is apparently; みたい is casual seems: 雨が降りそうです / 雨が降るそうです / 雨が降るようです / 雨が降るらしいです / 雨が降るみたいです。",
    "contrast-certainty": "はずだ is logical expectation; でしょう is probably; かもしれない is might: 彼は来るはずです / 彼は来るでしょう / 彼は来るかもしれません。",
    "contrast-te-iru-te-aru": "ている is a resulting state; てある is a state left on purpose: 窓が開いています / 窓が開けてあります。",
    "contrast-just-did": "たところ is the moment right after; たばかり is recently from the speaker's view and can stretch to weeks: 今着いたところです / 先週日本に来たばかりです（×先週来たところです）。",
    "contrast-tame-youni": "ために marks purpose with controllable actions; ように marks purpose so a state or ability holds: 合格するために勉強します / 忘れないようにノートに書きます。",
    "contrast-made-madeni": "まで means until (continuing through); までに means by a deadline: 五時まで勉強します / 五時までに宿題を出します。",
    "contrast-rareru": "The same られる shape is potential, passive, or honorific by context: 私は刺身が食べられます / ケーキを弟に食べられました / 先生はもう食べられました。",
    "contrast-voice": "Passive is done-to; causative is make/let do; causative-passive is being made to: 先生に褒められました / 子供に野菜を食べさせます / 野菜を食べさせられました。",
    "contrast-okage-sei": "おかげで is thanks to (good result); せいで is because of (blame); ために is formal cause: 先生のおかげで合格しました / 雨のせいで遅れました / 事故のために遅れました。",
    "contrast-topic-phrases": "について is about; に対して is toward or against; にとって is for from a standpoint: 日本の歴史について話します / 質問に対して答えます / 私にとって大切です。",
    "contrast-change": "につれて is as X changes, Y changes; 一方だ keeps getting more; ようになる is come to be able or become: 年を取るにつれて忘れやすくなります / 物価は上がる一方です / 日本語が話せるようになりました。",
    "contrast-concession": "のに is even though (unexpected); くせに is even though (critical); ながらも is although; にしては is for/considering: 勉強したのに忘れました / 学生のくせに生意気です / 小さいながらも強いです / 初心者にしては上手です。",
    "contrast-should": "べきだ is should (duty or logic); ほうがいい is advice; ことだ is the best thing is to: もっと運動するべきです / もっと運動したほうがいいです / もっと運動することです。",
    "contrast-wake": "わけだ means that's why; わけではない means it doesn't mean; わけがない means no way; わけにはいかない means can't very well: 三時間しか寝ていないなら、眠いわけです / 行きたくないわけではありません / 彼がうそをつくわけがありません / 今帰るわけにはいきません。",
    "contrast-bakari-dake-shika": "ばかり means nothing but; だけ means only; しか〜ない means only with a negative: ゲームばかりします / ゲームだけします / ゲームしかしません。",
}

SKETCH = [
    {
        "level": "N2",
        "target": "35–40 points (sketch)",
        "themes": "Formal connectors and written register: にもかかわらず, ものの, に際して, にあたって, をめぐって, をはじめ, に伴って; inevitability and emotion: ざるを得ない, ずにはいられない, ようがない, かねる; scope: に限らず, を問わず, ばかりに, ばかりか.",
        "existing": [
            {"slug": "ni-mo-kakawarazu", "title": "〜にもかかわらず", "status": "existing", "note": "Keep at N2"},
            {"slug": "mono-no", "title": "〜ものの", "status": "existing", "note": "Keep at N2"},
            {"slug": "zaru-o-enai", "title": "〜ざるを得ない", "status": "existing", "note": "Move N1→N2"},
            {"slug": "ni-kagirazu", "title": "〜に限らず", "status": "existing", "note": "Move N1→N2"},
            {"slug": "wo-towazu", "title": "〜を問わず", "status": "existing", "note": "Move N1→N2"},
        ],
    },
    {
        "level": "N1",
        "target": "20–30 points (sketch)",
        "themes": "Literary and formal: であれ, ともなると, をもって, をよそに, なりに, ずじまい, たところで, んばかり, ゆえに, に堪える.",
        "existing": [
            {"slug": "ni-taeru", "title": "〜に堪える", "status": "existing", "note": "Only point references agree is N1"},
        ],
    },
]


def parse_source(source: str) -> tuple[str, str | None]:
    """Return (status, level_override)."""
    if source == "new" or source.startswith("split:"):
        return "planned", None
    if source == "keep":
        return "existing", None
    m = re.fullmatch(r"keep:(N[1-5])→(N[1-5])", source)
    if m:
        # Unit level is the destination; caller sets level from unit.
        return "existing", m.group(2)
    raise ValueError(f"Unknown source: {source}")


def load_seed_slugs() -> set[str]:
    root = Path(__file__).resolve().parents[1]
    slugs: set[str] = set()
    for path in (root / "src/seed_data.rs", root / "src/seed_data_extra.rs"):
        text = path.read_text(encoding="utf-8")
        for m in re.finditer(r'slug:\s*"([^"]+)"', text):
            slugs.add(m.group(1))
    return slugs


def build() -> dict:
    seed = load_seed_slugs()
    missing_keep: list[str] = []
    units_out = []

    for u in UNITS:
        unit_id = f"u{u['n']:02d}"
        points_out = []

        for slug, title, teaches, needs, source in u["points"]:
            status, _level_override = parse_source(source)
            if status == "existing" and slug not in seed:
                missing_keep.append(slug)
            prereqs = [s for s in needs.split(",") if s]
            points_out.append(
                {
                    "slug": slug,
                    "title": title,
                    "summary": teaches,
                    "prerequisites": prereqs,
                    "status": status,
                    "level": u["level"],
                    "kind": "point",
                    "source": source,
                }
            )

        for contrast in u["contrasts"]:
            slug, _, rest = contrast.partition(":")
            slug = slug.strip()
            title = rest.strip()
            members = CONTRAST_MEMBERS.get(slug)
            if not members:
                raise SystemExit(f"Missing contrast member map for {slug}")
            summary = CONTRAST_SUMMARIES.get(slug)
            if not summary:
                raise SystemExit(f"Missing contrast summary for {slug}")
            # Validate members appear in this unit or earlier teaching (checked by Rust too)
            points_out.append(
                {
                    "slug": slug,
                    "title": title,
                    "summary": summary,
                    "prerequisites": members,
                    "status": "planned",
                    "level": u["level"],
                    "kind": "contrast",
                    "source": "new",
                }
            )

        units_out.append(
            {
                "id": unit_id,
                "n": u["n"],
                "level": u["level"],
                "title": u["title"],
                "goal": u["goal"],
                "sort_order": u["n"],
                "dojo": u["dojo"],
                "reading": u["reading"],
                "points": points_out,
            }
        )

    # Assign global teaching order across units (points including contrasts in order)
    sort_order = 0
    teaching_order = []
    for unit in units_out:
        for point in unit["points"]:
            sort_order += 1
            point["sort_order"] = sort_order
            teaching_order.append(point["slug"])

    # Copy sketch so module-level SKETCH is never mutated in place.
    sketch_out = json.loads(json.dumps(SKETCH))
    sketch_slugs = []
    for sketch in sketch_out:
        for p in sketch["existing"]:
            sketch_slugs.append(p["slug"])
            if p["slug"] not in seed:
                missing_keep.append(p["slug"])

    if missing_keep:
        missing = ", ".join(sorted(set(missing_keep)))
        raise SystemExit(
            f"Fail-fast: keep/existing slug(s) missing from seed_data: {missing}"
        )

    unit_and_sketch = set(teaching_order) | set(sketch_slugs)
    orphan_seeds = sorted(seed - unit_and_sketch)
    covered_existing = sorted(seed & set(teaching_order))
    covered_sketch = sorted(seed & set(sketch_slugs))

    return {
        "version": 1,
        "units": units_out,
        "sketch": sketch_out,
        "meta": {
            "seed_slug_count": len(seed),
            "curriculum_point_count": len(teaching_order),
            "missing_keep_slugs": [],
            "seeded_in_units": covered_existing,
            "seeded_in_sketch": covered_sketch,
            "orphan_seed_slugs": orphan_seeds,
        },
    }


def main() -> None:
    root = Path(__file__).resolve().parents[1]
    out = root / "content" / "curriculum.json"
    data = build()
    out.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    meta = data["meta"]
    print(f"Wrote {out}")
    print(f"units={len(data['units'])} points={meta['curriculum_point_count']}")
    print(f"missing_keep={meta['missing_keep_slugs']} orphan_seeds={meta['orphan_seed_slugs']}")
    print(f"seeded_in_sketch={meta['seeded_in_sketch']}")


if __name__ == "__main__":
    main()
