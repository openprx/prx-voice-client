# Early commit: measurements and the committed units with their translations

## Measurements (median / p95, one run per row, 2026-11-21)

Machine: the development Mac, with the translation server (Gemma 4 E4B, all layers on the GPU) and the harness running. Other work was running at the same time and loaded the machine: a virtual machine of another project at 100 to 400 percent CPU, another project's test binary at about 90 percent, `PerfPowerServices` at about 190 percent, load average 5 to 10 (see the `machine before` and `machine after` lines of each run). The absolute times are therefore on the pessimistic side; the comparison between the three behaviours is within the same conditions, run one after the other. Settings: the built-in `[pipeline]` defaults (`--default-pipeline`), `call.barge_in = true` unless stated, recognition mode `two_pass` as planned for this machine. Negative values mean the first translated audio began before the speaker finished.

The ordinary set is the earlier measurement set (10 read utterances and 5 number utterances per language). The long set is the benchmark's monologues (`long_zh`, `long_en`, 45 to 53 s), five `pause_*` items (multi-sentence read speech of 6 to 13 s) and four `conv_en` meeting items.

### Ordinary set, Mandarin source (outgoing)

| set | n | speech start to first translated audio, median / p95 (s) | speech end to first translated audio (s) | speech end to last translated audio finished (s) | committed units per utterance | skipped: backlog / barge-in stale | finals disagreeing with committed text |
|---|---|---|---|---|---|---|---|
| `segment` | 15 | 5.20 / 8.29 | 0.88 / 1.22 | 6.39 / 11.53 | 2.00 / 4.00 | 0 / 0 | 0 |
| `punctuation` | 15 | 4.34 / 5.89 | -0.70 / 1.09 | 5.95 / 10.05 | 2.00 / 4.00 | 0 / 0 | 1 |
| `chunk` | 15 | 3.46 / 4.78 | -1.30 / 0.94 | 6.00 / 7.83 | 3.00 / 5.00 | 0 / 0 | 2 |

### Ordinary set, English source (incoming)

| set | n | speech start to first translated audio, median / p95 (s) | speech end to first translated audio (s) | speech end to last translated audio finished (s) | committed units per utterance | skipped: backlog / barge-in stale | finals disagreeing with committed text |
|---|---|---|---|---|---|---|---|
| `segment` | 15 | 4.78 / 8.60 | 0.79 / 1.23 | 8.23 / 12.14 | 2.00 / 6.00 | 0 / 0 | 1 |
| `punctuation` | 15 | 4.78 / 8.50 | -1.56 / 1.21 | 6.57 / 15.53 | 2.00 / 6.00 | 0 / 1 | 2 |
| `chunk` | 15 | 3.19 / 8.52 | -4.65 / 0.97 | 6.15 / 14.75 | 4.00 / 6.00 | 0 / 0 | 2 |

### Longer utterances

| set | n | speech start to first translated audio, median / p95 (s) | speech end to first translated audio (s) | speech end to last translated audio finished (s) | committed units per utterance | skipped: backlog / barge-in stale | finals disagreeing with committed text |
|---|---|---|---|---|---|---|---|
| long_zh (2 monologues), `segment` | 2 | 8.38 / 8.49 | -33.56 / -30.42 | 13.61 / 20.64 | 9.50 / 10.00 | 0 / 4 | 1 |
| long_zh (2 monologues), `punctuation` | 2 | 3.73 / 4.83 | -38.21 / -36.06 | 12.90 / 18.86 | 15.00 / 16.00 | 0 / 3 | 1 |
| long_zh (2 monologues), `chunk` | 2 | 3.05 / 3.47 | -38.89 / -36.07 | 11.27 / 15.86 | 20.50 / 22.00 | 0 / 5 | 1 |
| pause_zh (5), `segment` | 5 | 4.86 / 6.79 | -3.10 / -1.22 | 8.47 / 10.01 | 4.00 / 4.00 | 0 / 0 | 0 |
| pause_zh (5), `punctuation` | 5 | 4.00 / 6.79 | -3.37 / -2.65 | 7.53 / 12.33 | 5.00 / 6.00 | 0 / 0 | 0 |
| pause_zh (5), `chunk` | 5 | 3.48 / 3.90 | -4.24 / -3.14 | 8.05 / 9.65 | 7.00 / 8.00 | 0 / 0 | 1 |
| long_en (2 monologues), `segment` | 2 | 7.08 / 8.37 | -40.39 / -34.76 | 11.53 / 12.09 | 13.00 / 18.00 | 0 / 4 | 0 |
| long_en (2 monologues), `punctuation` | 2 | 5.24 / 6.46 | -42.24 / -36.67 | 10.66 / 11.05 | 15.50 / 21.00 | 0 / 2 | 0 |
| long_en (2 monologues), `chunk` | 2 | 3.61 / 4.65 | -43.86 / -38.48 | 11.18 / 12.90 | 23.50 / 30.00 | 0 / 4 | 1 |
| pause_en (5), `segment` | 5 | 3.08 / 5.61 | -2.81 / -1.96 | 6.87 / 8.64 | 4.00 / 5.00 | 0 / 0 | 0 |
| pause_en (5), `punctuation` | 5 | 3.16 / 5.67 | -2.73 / -1.87 | 6.62 / 8.38 | 4.00 / 5.00 | 0 / 0 | 0 |
| pause_en (5), `chunk` | 5 | 3.07 / 3.38 | -5.02 / -1.94 | 5.53 / 6.95 | 4.00 / 5.00 | 0 / 0 | 0 |
| conv_en (4), `segment` | 4 | 3.40 / 4.37 | -0.19 / 0.98 | 5.64 / 8.80 | 2.00 / 2.00 | 0 / 0 | 0 |
| conv_en (4), `punctuation` | 4 | 2.71 / 3.23 | -1.01 / 0.06 | 5.33 / 6.54 | 2.00 / 3.00 | 0 / 0 | 0 |
| conv_en (4), `chunk` | 4 | 2.61 / 3.13 | -1.09 / -0.04 | 5.54 / 8.25 | 2.00 / 4.00 | 0 / 0 | 0 |
| long_zh, `chunk`, `call.barge_in = false` | 2 | 3.08 / 3.48 | -38.86 / -36.02 | 15.61 / 20.16 | 21.00 / 23.00 | 0 / 0 | 1 |
| long_en, `chunk`, `call.barge_in = false` | 2 | 3.67 / 4.69 | -43.80 / -38.44 | 12.98 / 16.18 | 23.50 / 30.00 | 0 / 0 | 1 |

### Numbers, dates, names and negation sets

| set | n | speech start to first translated audio, median / p95 (s) | speech end to first translated audio (s) | speech end to last translated audio finished (s) | committed units per utterance | skipped: backlog / barge-in stale | finals disagreeing with committed text |
|---|---|---|---|---|---|---|---|
| num_zh (10), `segment` | 10 | 4.20 / 5.42 | 0.88 / 1.04 | 5.54 / 6.55 | 1.00 / 1.00 | 0 / 0 | 0 |
| num_zh (10), `punctuation` | 10 | 4.19 / 5.39 | 0.86 / 1.01 | 5.67 / 6.92 | 1.00 / 1.00 | 0 / 0 | 0 |
| num_zh (10), `chunk` | 10 | 3.91 / 4.78 | 0.74 / 0.93 | 5.27 / 6.59 | 1.50 / 2.00 | 0 / 0 | 0 |
| num_en (10), `segment` | 10 | 4.16 / 5.46 | 0.88 / 1.04 | 4.84 / 6.40 | 1.00 / 1.00 | 0 / 0 | 0 |
| num_en (10), `punctuation` | 10 | 4.16 / 5.46 | 0.88 / 0.99 | 4.81 / 6.25 | 1.00 / 1.00 | 0 / 0 | 0 |
| num_en (10), `chunk` | 10 | 3.85 / 5.38 | 0.85 / 0.96 | 4.96 / 7.18 | 1.00 / 2.00 | 0 / 0 | 0 |
| own zh (5), `segment` | 5 | 6.21 / 8.49 | 0.95 / 1.06 | 6.89 / 10.43 | 1.00 / 2.00 | 0 / 0 | 0 |
| own zh (5), `punctuation` | 5 | 4.81 / 5.96 | -0.44 / 0.97 | 6.00 / 7.21 | 2.00 / 2.00 | 0 / 0 | 0 |
| own zh (5), `chunk` | 5 | 4.71 / 4.77 | -0.89 / 0.13 | 5.42 / 6.97 | 2.00 / 3.00 | 0 / 0 | 0 |
| own en (5), `segment` | 5 | 5.34 / 6.53 | 0.96 / 1.03 | 6.69 / 9.42 | 1.00 / 1.00 | 0 / 0 | 0 |
| own en (5), `punctuation` | 5 | 5.33 / 5.97 | 0.92 / 0.98 | 6.67 / 8.75 | 1.00 / 2.00 | 0 / 0 | 0 |
| own en (5), `chunk` | 5 | 3.06 / 3.53 | -1.39 / -0.76 | 4.95 / 7.39 | 2.00 / 3.00 | 0 / 0 | 0 |

# Committed units and their translations, side by side

Produced with `call_harness --default-pipeline --commit segment|punctuation|chunk` (real providers: Qwen3-ASR re-decode with the refine pass, Gemma 4 E4B, Qwen3-TTS; wav output). Each line is one committed unit: the recognised source text that was translated, then its translation. `segment` is `pipeline.early_commit = "segment_end"`. The recognised text is the recogniser's own and has its own errors (for example in the English read speech): compare the units and their translations, not the recognition. A trailing one-word unit such as `嗯。` or `I.` is the recogniser's echo in the silence the harness appends; it is present in all three behaviours.

With `chunk` and `punctuation` the units are the recogniser's stable text, which is not refined (see "Early commit and the refine pass" in the checklist).

## Numbers, dates, names and negation (own set, system-synthesised speech)

### own_zh0 (zh)

Reference: 我下周三下午三点不能参加会议，请改到周四上午十点。

- `segment`:
  - 我下周三下午三点不能参加会议，请改到周四上午十点。 => I can't attend the meeting next Wednesday at 3 PM, please change it to Thursday at 10 AM.
- `punctuation`:
  - 我下周三下午三点不能参加会议， => I can't attend the meeting next Wednesday at 3 PM.
  - 请改到周四上午十点。 => Please change it to Thursday at 10 AM.
- `chunk`:
  - 我下周三下午三点不能参加 => I can't make it next Wednesday at three in the afternoon.
  - 会议，请改到周四上午十点。 => Meeting, please change it to Thursday at ten in the morning.

### own_zh1 (zh)

Reference: 王建国说他不是昨天到的，而是三月十八号晚上到的。

- `segment`:
  - 王建国说：“他不是昨天到的，而是三月十八号晚上到的。” => Wang Jianguao said, "He didn't arrive yesterday, but on the evening of March 18th."
- `punctuation`:
  - 王建国说：“他不是昨天到的， => Wang Jianguao said, "He didn't arrive yesterday,"
  - 而是三月十八号晚上到的。” => but arrived on the evening of March 18th."
- `chunk`:
  - 王建国说：“ => Wang Jianguao said:
  - 他不是昨天到的， => He didn't arrive yesterday,
  - 而是三月十八号晚上到的。” => but arrived on the evening of March 18th.

### own_zh2 (zh)

Reference: 这个项目的预算是二百五十万元，我们还没有批准。

- `segment`:
  - 这个项目的预算是二百五十万元，我们还没有批准。 => The budget for this project is 2.5 million yuan, and we haven't approved it yet.
- `punctuation`:
  - 这个项目的预算是二百五十万元， => The budget for this project is 2.5 million yuan.
  - 我们还没有批准。 => We haven't approved it yet.
- `chunk`:
  - 这个项目的预算是二百五十万元， => The budget for this project is 2.5 million yuan.
  - 我们还没有批准。 => We haven't approved it yet.

### own_zh3 (zh)

Reference: 请把一百零八号房间的钥匙交给李明，不要给别人。

- `segment`:
  - 请把一百零八号房间的钥匙交给李明，不要给别人。 => Please give the key to room 108 to Li Ming, don't give it to anyone else.
- `punctuation`:
  - 请把一百零八号房间的钥匙交给李明，不要给别人。 => Please give the key to room 108 to Li Ming, don't give it to anyone else.
- `chunk`:
  - 请把一百零八号房间的钥匙 => The key to room 108
  - 交给李明，不要给别人。 => give to Li Ming, don't give it to anyone else.

### own_zh4 (zh)

Reference: 我们的航班是三十七号，起飞时间是二零二六年十二月二日早上六点四十五分。

- `segment`:
  - 我们的航班是三十七号，起飞时间是二零二六年十二月二日早上 => Our flight is on the 37th, and the departure time is 6:45 AM on December 2, 2026.
  - 六点四十五分。 => Mm.
- `punctuation`:
  - 我们的航班是三十七号， => Our flight is on the 37th.
  - 起飞时间是二零二六年十二月二日早上六点四十五分。 => The departure time is 6:45 AM on December 2, 2026.
- `chunk`:
  - 我们的航班是三十七号， => Our flight is on the 37th.
  - 起飞时间是二零二六年十二月二日 => The departure time is December 2nd, 2026.
  - 早上六点四十五分。 => 6:45 in the morning.

### own_en0 (en)

Reference: Please send me the report by 5 PM on March 3rd, not tomorrow.

- `segment`:
  - Please send me the report by 5 p.m. on March 3rd. Not tomorrow. => 请在3月3日下午5点前把报告发给我。不是明天。
- `punctuation`:
  - Please send me the report by 5 p.m. on March 3rd. Not tomorrow. => 请在3月3日下午5点前把报告发给我。不是明天。
- `chunk`:
  - Please send me => 请发给我
  - the report by 5 p.m. on March 3rd. Not tomorrow. => 请在3月3日下午5点前把报告发给我。不是明天。

### own_en1 (en)

Reference: Doctor Patel said I should not take more than two tablets a day.

- `segment`:
  - Dr. Patel said, "I should not take more than two tablets a day." => 帕特尔医生说我一天不应该吃超过两片。
- `punctuation`:
  - Dr. Patel said, "I should not take more than two tablets a day." => 帕特尔医生说我一天不应该吃超过两片。
- `chunk`:
  - Dr. Patel said, => 帕特尔医生说，
  - "I should not take more than two tablets a day." => 我一天不应该吃超过两片。

### own_en2 (en)

Reference: The meeting with Anna Schmidt moved from room 204 to room 315.

- `segment`:
  - The meeting with Anna Schmidt moved from Room 204 to Room 315. => 和安娜·施密特的会议从204室改到315室了。
- `punctuation`:
  - The meeting with Anna Schmidt moved from Room 204 to Room 315. => 和安娜·施密特的会议从204室改到315室了。
- `chunk`:
  - The meeting with Anna Schmidt => 和安娜·施密特的会议
  - moved from Room 204 to Room 315. => 从204室搬到了315室。

### own_en3 (en)

Reference: I did not say the price was 1,500 dollars; I said it was 15,000.

- `segment`:
  - I did not say the price was $1,500. I said it was $15,000. => 我没说价格是1500美元。我说的是15000。
- `punctuation`:
  - I did not say the price was $1,500. I said it was $15,000. => 我没说价格是1500美元。我说的是15000。
- `chunk`:
  - I did not say the price => 我没有说价格
  - was $1,500. => 是1,500美元。
  - I said it was $15,000. => 我说的是15,000美元。

### own_en4 (en)

Reference: Our flight leaves on December 2nd at 6:45 in the morning, and it never arrives before noon.

- `segment`:
  - Our flight leaves on December 2nd at 6:45 in the morning, and it never arrives before noon. => 我们的航班是12月2号早上6点45分起飞，而且它从来没有在中午前到达过。
- `punctuation`:
  - Our flight leaves on December 2nd at 6:45 in the morning, => 我们的航班是12月2号早上6点45分起飞，
  - and it never arrives before noon. => 而且它从来没有在中午之前到达。
- `chunk`:
  - Our flight leaves => 我们的航班起飞
  - on December 2nd at 6:45 in the morning, => 在12月2日早上6点45分，
  - and it never arrives before noon. => 而且它从来没有在中午之前到达。

## Numbers, dates and names (synthetic set, system-synthesised speech)

### num_zh/nz00 (zh)

Reference: 二零二六年十一月二十日下午三点十五分开会。

- `segment`:
  - 二零二六年十一月二十日下午三点十五分开会。 => The meeting will be held at 3:15 PM on November 20, 2026.
- `punctuation`:
  - 二零二六年十一月二十日下午三点十五分开会。 => The meeting will be held at 3:15 PM on November 20, 2026.
- `chunk`:
  - 二零二六年十一月二十日 => November 20th, 2026
  - 下午三点十五分开会。 => The meeting will be adjourned at 3:15 PM.

### num_zh/nz01 (zh)

Reference: 请转账三万五千六百元到我的账户。

- `segment`:
  - 请转账三万五千六百元到我的账户。 => Please transfer 35,600 yuan to my account.
- `punctuation`:
  - 请转账三万五千六百元到我的账户。 => Please transfer 35,600 yuan to my account.
- `chunk`:
  - 请转账三万五千六百元到我的账户。 => Please transfer 35,600 yuan to my account.

### num_zh/nz02 (zh)

Reference: 我的电话号码是一三八零零一三八零零零。

- `segment`:
  - 我的电话号码是13800138000。 => My phone number is 13800138000.
- `punctuation`:
  - 我的电话号码是13800138000。 => My phone number is 13800138000.
- `chunk`:
  - 我的电话号码是13800138000。 => My phone number is 13800138000.

### num_zh/nz03 (zh)

Reference: 这个项目的预算是两百五十万美元。

- `segment`:
  - 这个项目的预算是两百五十万美元。 => The budget for this project is two million five hundred thousand US dollars.
- `punctuation`:
  - 这个项目的预算是两百五十万美元。 => The budget for this project is two million five hundred thousand US dollars.
- `chunk`:
  - 这个项目的预算是两百五十万美元。 => The budget for this project is two million five hundred thousand US dollars.

### num_zh/nz04 (zh)

Reference: 会议在北京时间二月十四号早上九点半开始。

- `segment`:
  - 会议在北京时间二月十四号早上九点半开始。 => The meeting starts at 9:30 AM Beijing time on February 14th.
- `punctuation`:
  - 会议在北京时间二月十四号早上九点半开始。 => The meeting starts at 9:30 AM Beijing time on February 14th.
- `chunk`:
  - 会议在北京时间二月十四号 => The meeting is on February 14th Beijing time.
  - 早上九点半开始。 => Starting at 9:30 AM.

### num_zh/nz05 (zh)

Reference: 苹果公司的首席执行官是蒂姆库克。

- `segment`:
  - 苹果公司的首席执行官是蒂姆·库克。 => Apple's CEO is Tim Cook.
- `punctuation`:
  - 苹果公司的首席执行官是蒂姆·库克。 => Apple's CEO is Tim Cook.
- `chunk`:
  - 苹果公司的首席执 => Apple's CEO
  - 行官是蒂姆·库克。 => The CEO is Tim Cook.

### num_zh/nz06 (zh)

Reference: 他出生于一九八五年七月三日。

- `segment`:
  - 他出生于一九八五年七月三日。 => He was born on July 3, 1985.
- `punctuation`:
  - 他出生于一九八五年七月三日。 => He was born on July 3, 1985.
- `chunk`:
  - 他出生于一九八五年七月三日。 => He was born on July 3, 1985.

### num_zh/nz07 (zh)

Reference: 航班号是国航一二三四，座位二十三A。

- `segment`:
  - 航班号是国航一二三四，座位二十三A。 => The flight number is Air China 1234, seat 23A.
- `punctuation`:
  - 航班号是国航一二三四，座位二十三A。 => The flight number is Air China 1234, seat 23A.
- `chunk`:
  - 航班号是国航一二三四， => The flight number is Air China 1234.
  - 座位二十三A。 => Seat 23A.

### num_zh/nz08 (zh)

Reference: 总共一千二百三十四点五六元。

- `segment`:
  - 总共一千二百三十四点五六元。 => A total of one thousand two hundred thirty-four point five six yuan.
- `punctuation`:
  - 总共一千二百三十四点五六元。 => A total of one thousand two hundred thirty-four point five six yuan.
- `chunk`:
  - 总共一千二百三十四点五六元。 => A total of one thousand two hundred thirty-four point five six yuan.

### num_zh/nz09 (zh)

Reference: 请联系张伟经理或者李娜女士。

- `segment`:
  - 请联系张伟经理或者李娜女士。 => Please contact Manager Zhang Wei or Ms. Li Na.
- `punctuation`:
  - 请联系张伟经理或者李娜女士。 => Please contact Manager Zhang Wei or Ms. Li Na.
- `chunk`:
  - 请联系张伟经理 => Please contact Manager Zhang Wei.
  - 或者李娜女士。 => Or Ms. Li Na.

### num_en/ne00 (en)

Reference: The meeting is on November twentieth, twenty twenty six at three fifteen p m.

- `segment`:
  - The meeting is on November 20th, 2026 at 3:15 p.m. => 会议在2026年11月20日下午3点15分。
- `punctuation`:
  - The meeting is on November 20th, 2026 at 3:15 p.m. => 会议在2026年11月20日下午3点15分。
- `chunk`:
  - The meeting is on November 20th, 2026 at 3:15 p.m. => 会议在2026年11月20日下午3点15分。

### num_en/ne01 (en)

Reference: Please transfer thirty five thousand six hundred dollars to my account.

- `segment`:
  - Please transfer $35,600 to my account. => 请把三万五千六百美元转到我的账户。
- `punctuation`:
  - Please transfer $35,600 to my account. => 请把三万五千六百美元转到我的账户。
- `chunk`:
  - Please transfer $35,600 to my account. => 请把三万五千六百美元转到我的账户。

### num_en/ne02 (en)

Reference: My phone number is five five five, zero one two three.

- `segment`:
  - My phone number is 555-0123. => 我的电话号码是五五五零一二三。
- `punctuation`:
  - My phone number is 555-0123. => 我的电话号码是五五五零一二三。
- `chunk`:
  - My phone number => 我的电话号码
  - is 555-0123. => 是555-0123。

### num_en/ne03 (en)

Reference: The total comes to one thousand two hundred thirty four dollars and fifty six cents.

- `segment`:
  - The total comes to $1,234.56. => 总共是1234美元56美分。
- `punctuation`:
  - The total comes to $1,234.56. => 总共是1234美元56美分。
- `chunk`:
  - The total comes => 总共是
  - to $1,234.56. => 1,234.56美元。

### num_en/ne04 (en)

Reference: Our flight number is United four one seven, seat twenty three A.

- `segment`:
  - Our flight number is United 417 C23A. => 我们的航班号是United 417。座位是23A。
- `punctuation`:
  - Our flight number is United 417 C23A. => 我们的航班号是United 417。座位是23A。
- `chunk`:
  - Our flight number is United 417 C23A. => 我们的航班号是United 417。座位是23A。

### num_en/ne05 (en)

Reference: She was born on July third, nineteen eighty five.

- `segment`:
  - She was born on July 3, 1985. => 她出生于1985年7月3日。
- `punctuation`:
  - She was born on July 3, 1985. => 她出生于1985年7月3日。
- `chunk`:
  - She was born on July 3, 1985. => 她出生于1985年7月3日。

### num_en/ne06 (en)

Reference: The chief executive of Microsoft is Satya Nadella.

- `segment`:
  - The chief executive of Microsoft is Satya Nadella. => 微软的首席执行官是萨蒂亚·纳德拉。
- `punctuation`:
  - The chief executive of Microsoft is Satya Nadella. => 微软的首席执行官是萨蒂亚·纳德拉。
- `chunk`:
  - The chief executive of Microsoft is Satya Nadella. => 微软的首席执行官是萨蒂亚·纳德拉。

### num_en/ne07 (en)

Reference: We raised two point five million dollars in March.

- `segment`:
  - We raised 2.5 million dollars in March. => 我们三月份筹集了两百五十万美元。
- `punctuation`:
  - We raised 2.5 million dollars in March. => 我们三月份筹集了两百五十万美元。
- `chunk`:
  - We raised 2.5 million dollars in March. => 我们三月份筹集了两百五十万美元。

### num_en/ne08 (en)

Reference: Please contact Doctor Johnson or Miss Katherine Williams.

- `segment`:
  - Please contact Dr. Johnson or Miss Catherine Williams. => 请联系 Johnson 医生或 Catherine Williams 小姐。
- `punctuation`:
  - Please contact Dr. Johnson or Miss Catherine Williams. => 请联系 Johnson 医生或 Catherine Williams 小姐。
- `chunk`:
  - Please contact Dr. Johnson or Miss Catherine Williams. => 请联系 Johnson 医生或 Catherine Williams 小姐。

### num_en/ne09 (en)

Reference: The package weighs about twelve point five kilograms.

- `segment`:
  - The package weighs about 12.5 kilograms. => 这个包裹重大概12.5公斤。
- `punctuation`:
  - The package weighs about 12.5 kilograms. => 这个包裹重大概12.5公斤。
- `chunk`:
  - The package weighs about 12.5 kilograms. => 这个包裹重大概12.5公斤。

## Read speech (first items of the ordinary set)

### zh_read/zh000 (zh)

Reference: 在独立战争期间，十三个州最初根据《联邦条例》组成了弱小的中央政府（国会是其唯一的组成部分）。

- `segment`:
  - 在独立战争期间，是三个州根据联邦条例组成了弱小的中央政府，国会是其唯一的组成部分。 => During the American Revolutionary War, the three states formed a weak central government according to the federal regulations, and Congress was its only component.
  - 嗯。 => Uh.
- `punctuation`:
  - 在独立战争期间， => During the American Revolutionary War,
  - 十三个州根据联邦条例组成了弱小的中央政府， => The thirteen states formed a weak central government under the Articles of Confederation,
  - 国会是其唯一的组成部分。 => Congress was its only component.
  - 嗯。 => Uh.
- `chunk`:
  - 在独立战争期间， => During the American Revolutionary War,
  - 十三个州根据联邦条 => the thirteen states according to the federal
  - 例组成了弱小的中央政府， => formed a weak central government,
  - 国会是其唯一的组成部分。 => Congress was its only component.
  - 嗯。 => Uh.

### zh_read/zh001 (zh)

Reference: 跳羚队以五连败收官。

- `segment`:
  - 跳羚队以五连败收官。 => The Jumping Ant team finished with five consecutive losses.
  - 嗯。 => Mm.
- `punctuation`:
  - 跳羚队以五连败收官。 => The Jumping Ant team finished with five consecutive losses.
  - 嗯。 => Mm.
- `chunk`:
  - 跳羚队以五连败收官。 => The Jumping Ant team finished with five consecutive losses.
  - 嗯。 => Mm.

### zh_read/zh002 (zh)

Reference: 孙德尔本斯 (Sundarbans) 已经被联合国教科文组织宣布为世界遗产。印度境内的这部分森林被称为孙德尔本斯国家公园。

- `segment`:
  - 孙德尔本斯已经被联合国教科文组织宣布为世界遗产。 => Sundarbans has been declared a World Heritage Site by UNESCO.
  - 印度境内的这部分森林被称为孙德尔本斯国家公园。 => This part of the forest within India is called Sundarbans National Park.
- `punctuation`:
  - 孙德尔本斯已经被联合国教科文组织宣布为世界遗产。 => Sundarbans has been declared a World Heritage Site by UNESCO.
  - 印度境内的这部分森林被称为孙德尔本斯国家公园。 => This part of the forest within India is called Sundarbans National Park.
- `chunk`:
  - 孙德尔本斯已经 => Sunderson has already
  - 被联合国教科文组织宣布为世界遗产。 => been declared a World Heritage site by UNESCO.
  - 印度境内的这部分森 => This part of the forest in India
  - 林被称为孙德尔本斯国家公园。 => is called Sunderson National Park.

### zh_read/zh003 (zh)

Reference: 阿皮亚是萨摩亚的首都。该城镇位于乌波卢（Upolu）岛上，人口数量不足 4 万。

- `segment`:
  - 阿皮亚是萨摩亚的首都。 => Apia is the capital of Samoa.
  - 该城镇位于。 => The town is located in.
  - 乌波鲁岛上。 => on Upolu Island.
  - 人口数量不足四万。 => The population is less than 40,000.
- `punctuation`:
  - 阿皮亚是萨摩亚的首都。 => Apia is the capital of Samoa.
  - 该城镇位于。 => The town is located in.
  - 乌波鲁岛上。 => on Upolu Island.
  - 人口数量不足四万。 => The population is less than 40,000.
- `chunk`:
  - 阿皮亚是萨摩亚的首都。 => Apia is the capital of Samoa.
  - 该城镇位于。 => The town is located in.
  - 乌波鲁岛上。 => on Upolu Island.
  - 人口数量不足四万。 => The population is less than 40,000.

### zh_read/zh004 (zh)

Reference: 另一方面，冰雪天气在很多国家都十分常见，全年交通也几乎不会中断。

- `segment`:
  - 另一方面，冰雪天气在许多国家都非十分常见，全年交通也几乎不会中断。 => On the other hand, icy weather is not very common in many countries, and traffic almost never gets interrupted throughout the year.
  - 嗯。 => Mm.
- `punctuation`:
  - 另一方面，冰雪天气在许多国家都非十分常见， => On the other hand, icy and snowy weather is not very common in many countries.
  - 全年交通也几乎不会中断。 => Traffic almost never stops throughout the year either.
  - 嗯。 => Mm.
- `chunk`:
  - 另一方面，冰雪天气 => On the other hand, icy weather
  - 在许多国家都非十分常 => is not very common in many countries
  - 见，全年交通也几乎不会中断。 => See, traffic almost never stops throughout the year.
  - 嗯。 => Mm.

### en_read/en000 (en)

Reference: The harbor was the site of an infamous naval standoff in 1889 when seven ships from Germany, the US, and Britain refused to leave the harbor.

- `segment`:
  - The harbor was the site of an infamous. => 那个港口是臭名昭著的地点。
  - Naval standoff. => 海军对峙。
  - In 1889, when seven ships from Germany, the U.S. and Britain refused to leave the harbor. => 在1889年，当来自德国、美国和英国的七艘船拒绝离开港口时，
- `punctuation`:
  - The harbor was the site of an infamous. => 那个港口是臭名昭著的地点。
  - Naval standoff. => 海军对峙。
  - In 1889, when seven ships from Germany, => 在1889年，当来自德国的七艘船，
  - the U.S. and Britain refused to leave the harbor. => 美国和英国拒绝离开港口。
- `chunk`:
  - The harbor was the site of an infamous. => 那个港口是臭名昭著的地点。
  - Naval standoff. => 海军对峙。
  - In 1889, when seven ships => 在1889年，当七艘船
  - from Germany, the U.S. and Britain refused to leave the harbor. => 来自德国，美国和英国拒绝离开港口。

### en_read/en001 (en)

Reference: The Report is highly critical of almost every aspect of the present policy of the Executive towards Iraq and it urges an immediate change of direction.

- `segment`:
  - The report is highly critical of the almost every aspect of the present policy of the executive towards Iraq, and it => 这份报告对行政部门目前对伊拉克政策的几乎每一个方面都持高度批评态度，并敦促立即改变方向。
  - urges an immediate change of direction. => 行动。
- `punctuation`:
  - The report is highly critical of the almost every aspect of the present policy of the executive towards Iraq, => 这份报告对行政部门目前对伊拉克政策的几乎每一个方面都持高度批评态度。
  - and it urges an immediate change of direction. => 并且它敦促立即改变方向。
- `chunk`:
  - The report is highly => 这份报告非常
  - critical of the almost every aspect => 批评几乎每一个方面
  - of the present policy => 的现有政策
  - of the executive towards Iraq, => 对伊拉克执行的
  - and it urges an immediate change of direction. => 并且敦促立即改变方向。

### en_read/en002 (en)

Reference: First among its 78 recommendations is that a new diplomatic initiative should be taken before the end of this year to secure Iraq’s borders against hostile interventions and to re-establish diplomatic relations with its neighbors.

- `segment`:
  - First among is seventy eight recommendations is that a => 在它提出的七十八项建议中，第一项是应该在今年年底前采取一项新的外交倡议来确保。
  - diplomatic initiative, should be taken before the end of this year to secure Iraq's borders against hostile hostile interventions, and to => 以确保伊拉克边境免受敌对干预，并恢复与邻国的外交关系。
  - re-establish diplomatic relations with its neighbors. => 宇宙。
- `punctuation`:
  - First among is seventy => 在它提出的78项建议中，第一项是应该在今年年底之前采取一项新的外交倡议。
  - recommendations is that a new diplomatic initiative, => 建议是应该采取一项新的外交倡议，
  - should be taken before the end of this year to secure Iraq's borders against hostile interventions, => 应该在今年年底之前采取措施，以确保伊拉克边境免受敌对干预。
  - and to re-establish diplomatic relations with its neighbors. => 并且与邻国重新建立外交关系。
- `chunk`:
  - First among is seventy => 在它提出的78项建议中，第一项是应该在今年年底之前采取一项新的外交倡议。
  - recommendations is that a new diplomatic initiative should be taken before the end => 建议是应该在年底之前采取一项新的外交倡议
  - d of this year => 的今年
  - to secure Iraq's borders against => 以确保伊拉克边境免受
  - hostile interventions and to re-establish diplomatic relations with its neighbors. => 敌对干预，并与邻国恢复外交关系。

### en_read/en003 (en)

Reference: In 2002 Goma was destroyed by lava from the Nyiragongo volcano which buried most of the town’s streets, particularly the town centre.

- `segment`:
  - In 2002, Goma was destroyed by lava from the => 2002年，戈马被恩吉拉贡火山的熔岩摧毁，大部分街道，特别是市中心都被埋了。
  - Nyanogo volcano, which buried most of the town's streets, particularly the town center. => 中心。
- `punctuation`:
  - In 2002, Goma was destroyed by lava from the Nyungabo volcano, => 2002年，戈马被恩翁博火山的熔岩摧毁了，
  - which buried most of the town's streets, particularly the town center. => 埋葬了大部分街道，特别是镇中心。
- `chunk`:
  - In 2002, Goma => 2002年，戈马
  - was destroyed by lava => 被熔岩摧毁了
  - from the Nyungabo volcano, => 来自恩翁博火山，
  - which volcano, which buried most => 哪个火山，埋得最多
  - of the town's streets, particularly the town center. => 的街道，特别是镇中心。

### en_read/en004 (en)

Reference: The capital of Moldova is Chişinău. The local language is Romanian, but Russian is widely used.

- `segment`:
  - The capital of Mendoza is Chichen Itza. => 摩尔多瓦的首都基希讷乌。
  - . => 是啊。
  - The local language is Romanian. => 当地语言是罗马尼亚语。
  - I. => 是啊。
  - But Russian is widely used. => 但是俄语被广泛使用。
  - I'm. => 是啊。
- `punctuation`:
  - The capital of Mendoza is Chichen Itza. => 摩尔多瓦的首都基希讷乌。
  - . => 是啊。
  - The local language is Romanian. => 当地语言是罗马尼亚语。
  - I. => 是啊。
  - But Russian is widely used. => 但是俄语被广泛使用。
  - I'm. => 是啊。
- `chunk`:
  - The capital of Mendoza is Chichen Itza. => 摩尔多瓦的首都基希讷乌。
  - . => 是啊。
  - The local language is Romanian. => 当地语言是罗马尼亚语。
  - I. => 是啊。
  - But Russian is widely used. => 但是俄语被广泛使用。
  - I'm. => 是啊。

