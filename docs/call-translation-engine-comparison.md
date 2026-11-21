# Translation engines compared: system translation and the language model

Status: measurement record, 2026-11-21. Machine: Apple M5 Max, macOS 26.6.2. Nothing was played or recorded.
Engines: system translation (Apple's on-device Translation framework, default strategy, packs zh-Hans and en
installed) and the language model (Gemma 4 E4B QAT Q4_0 on the owner's own  on port 8080, two slots,
the prompt and settings of the repository at the commit this was measured on). Both engines were built by the
provider registry, as a call builds them; the language model's server was the owner's, which the application only
used (never started or stopped).

## 1. Latency through the pipeline (`call_harness`, silent, WAV output)

The standard utterance sets of `bench/asr/data` (10 read sentences and the 5 synthetic number sentences of each
language, 15 utterances per run), real-time pace, recognition two-pass, commit `chunk`, one direction per run:
`call_harness --direction outgoing --set zh --engine system|llm` and `--direction incoming --set en --engine ...`.
Median / p95, seconds. No utterance had a failed clause, a suspect translation, a skipped clause or an underrun in any run.

| metric | zh to en, system | zh to en, model | en to zh, system | en to zh, model |
|---|---|---|---|---|
| MT time to first translated text | 0.06 / 0.10 | 0.10 / 0.14 | 0.06 / 0.11 | 0.11 / 0.21 |
| commit to first translated audio | 0.24 / 0.53 | 0.36 / 0.60 | 0.31 / 0.39 | 0.43 / 1.65 |
| speech start to first translated audio | 3.32 / 4.62 | 3.56 / 4.77 | 3.37 / 8.19 | 3.04 / 6.65 |
| speech end to first translated audio | -1.28 / 0.78 | -1.14 / 0.93 | -4.41 / 0.77 | -4.51 / 2.23 |
| speech end to last translated audio finished | 5.86 / 6.83 | 5.56 / 8.17 | 5.54 / 13.14 | 6.25 / 8.22 |

Reading it honestly:

- The engine's own share is small next to recognition and synthesis. Per committed clause the system engine delivers
  its text about 40 to 100 ms after the commit; the model needs about 80 to 110 ms for its first token and 200 to 300 ms
  for the whole sentence. Commit to first translated audio is about 0.12 s shorter with the system engine in both
  directions (0.24 against 0.36 s and 0.31 against 0.43 s, medians).
- Speech start to first translated audio is dominated by when the pipeline commits a clause (the chunk rule), not by the
  engine: the medians differ by less than the run-to-run noise (3.32 against 3.56 s; the English run is 3.37 against
  3.04 s in the model's favour). Do not read the 0.2 s differences in these rows as an engine effect.
- The machine was not quiet: other work on it (virtual machines, builds, tests) gave load averages of 22.7, 10.9, 7.0 and
  5.4 at the start of the four runs, in the order of the columns. The p95 columns of the English runs (8.19 s and 6.65 s)
  come from the long utterances and from that load. These are single runs of 15 utterances, not a benchmark.
- The first call of the system engine takes about 0.6 s cold; the registry absorbs it by warming the session up when the
  call is prepared (the warm-up, session preparation plus one short translation, measured 120 to 450 ms), so no clause pays it.

## 2. Translation quality and per-sentence time, side by side

48 sentences per direction in seven kinds (conversational, numbers, dates and times, names, negation, ambiguous business
words, incomplete clauses), produced by `prxvc-dev translate-compare`. Each sentence is one request with no context
for both engines (the system engine has none, so the model gets none either; in a call the model additionally sees the
previous clauses, which helps it). The sentences are in `apps/desktop/src/bin/prxvc_dev/sentences.rs`.

### Chinese to English and English to Chinese

#### Chinese to English (the outgoing direction)

| # | kind | source | system translation | language model | system ms | model first / total ms |
|---|---|---|---|---|---|---|
| 1 | conversational | 你好，能听到我说话吗？ | Hello, can you hear me? | Hello, can you hear me? | 58 | 28 / 96 |
| 2 | conversational | 不好意思，刚才网络有点卡，你再说一遍好吗？ | Sorry, the network was a little stuck just now. Can you say it again? | Sorry, the network was a bit laggy just now, could you say that again? | 99 | 113 / 292 |
| 3 | conversational | 我们先从上周的数据开始，然后再看这周的进展。 | Let's start with last week's data, and then look at the progress of this week. | Let's start with last week's data, and then look at this week's progress. | 78 | 74 / 279 |
| 4 | conversational | 这个周末你有空吗？我们一起吃个饭吧。 | Are you free this weekend? Let's have a meal together. | Are you free this weekend? Let's grab a meal together. | 70 | 77 / 218 |
| 5 | conversational | 好的，没问题，我马上发给你。 | OK, no problem. I'll send it to you right away. | Okay, no problem, I'll send it to you right away. | 68 | 79 / 234 |
| 6 | conversational | 你觉得这个方案怎么样？我个人觉得还不错。 | What do you think of this plan? Personally, I think it's not bad. | What do you think of this plan? Personally, I think it's not bad. | 86 | 83 / 272 |
| 7 | conversational | 谢谢你的耐心解释，我现在明白了。 | Thank you for your patient explanation. I understand now. | Thank you for your patient explanation, I understand now. | 71 | 86 / 203 |
| 8 | conversational | 我这边信号不太好，可能得换个地方再打给你。 | The signal here is not very good, so I may have to call you in another place. | My signal isn't very good here, I might have to find another place to call you from. | 87 | 89 / 313 |
| 9 | numbers | 这个项目的预算是三百五十万，不是五百三十万。 | The budget of this project is 3.5 million, not 5.30 million. | The budget for this project is 3.5 million, not 5.3 million. | 79 | 105 / 302 |
| 10 | numbers | 一共有十二个人参加，其中七个是新同事。 | A total of twelve people participated, seven of whom were new colleagues. | There are twelve people participating in total, seven of whom are new colleagues. | 85 | 96 / 267 |
| 11 | numbers | 报价是每台两千四百八十元，一百台起订。 | The quotation is 2,480 yuan per unit, and the minimum order is 100 units. | The quote is 2,480 yuan per unit, with a minimum order of 100 units. | 104 | 111 / 368 |
| 12 | numbers | 增长率是百分之十五点三，比去年高了两个百分点。 | The growth rate is 15.3%, two percentage points higher than last year. | The growth rate is fifteen point three percent, which is two percentage points higher than last year. | 89 | 112 / 313 |
| 13 | numbers | 请把电话号码一三八，零零一三，八零零零记下来。 | Please write down the phone numbers 138, 0013, 8000. | Please write down the phone number 138-0013-8000. | 106 | 97 / 331 |
| 14 | numbers | 合同总额大约一千二百万美元，分三期支付。 | The total amount of the contract is about 12 million US dollars, which will be paid in three installments. | The total contract amount is about twelve million US dollars, paid in three installments. | 99 | 108 / 271 |
| 15 | numbers | 会议室在三楼，房间号是三零八。 | The conference room is on the third floor, and the room number is 308. | The meeting room is on the third floor, room number is 308. | 83 | 104 / 273 |
| 16 | dates | 会议改到下周三下午三点十五分。 | The meeting has been changed to 3:15 p.m. next Wednesday. | The meeting has been changed to next Wednesday at 3:15 PM. | 83 | 101 / 256 |
| 17 | dates | 我三月三号到上海，四号下午回北京。 | I arrived in Shanghai on March 3rd and returned to Beijing on the afternoon of March 4th. | I arrive in Shanghai on March 3rd and return to Beijing on the afternoon of the 4th. | 82 | 87 / 312 |
| 18 | dates | 请在十一月三十号之前提交报告。 | Please submit the report before November 30th. | Please submit the report before November 30th. | 62 | 84 / 193 |
| 19 | dates | 下午三点十五分开会。 | The meeting will be separated at 3:15 p.m. | The meeting will be split at 3:15 PM. | 56 | 93 / 208 |
| 20 | dates | 我们大概需要两到三个星期来完成测试。 | It will take us about two to three weeks to complete the test. | We probably need two to three weeks to complete the testing. | 53 | 81 / 198 |
| 21 | dates | 发货时间是下个月月底，最晚不超过五号。 | The delivery time is the end of next month, no later than the 5th. | The shipping time is the end of next month, no later than the 5th. | 61 | 94 / 256 |
| 22 | dates | 早上九点半的航班，你得提前两个小时到机场。 | For the 9:30 a.m. flight, you have to arrive at the airport two hours in advance. | For the 9:30 AM flight, you have to be at the airport two hours early. | 57 | 83 / 286 |
| 23 | names | 王建国说他明天会和李明一起来拜访你们。 | Wang Jianguo said that he would visit you with Li Ming tomorrow. | Wang Jianguao said he will come to visit you with Li Ming tomorrow. | 55 | 83 / 238 |
| 24 | names | 我是张伟，在华为的采购部工作。 | I'm Zhang Wei, working in Huawei's Purchasing Department. | I am Zhang Wei, and I work in the purchasing department at Huawei. | 55 | 83 / 228 |
| 25 | names | 请转告陈经理，客户已经在大厅等他了。 | Please tell Manager Chen that the customer is already waiting for him in the lobby. | Please tell Manager Chen that the client is waiting for him in the lobby. | 56 | 83 / 230 |
| 26 | names | 这位是来自微软的Sarah，她负责云服务。 | This is Sarah from Microsoft. She is in charge of cloud services. | This is Sarah from Microsoft, she's in charge of cloud services. | 58 | 84 / 231 |
| 27 | names | 我们下个月要去慕尼黑参加汉诺威工业展。 | We are going to Munich to attend the Hanover Industrial Fair next month. | We are going to Munich next month to attend the Hannover Industrial Fair. | 55 | 85 / 224 |
| 28 | names | 周教授的论文发表在《自然》杂志上。 | Professor Zhou's paper was published in Nature magazine. | Professor Zhou's paper was published in Nature. | 56 | 86 / 189 |
| 29 | negation | 我不是说这个方案不行，我是说时间太紧了。 | I'm not saying that this plan doesn't work, I mean that the time is too tight. | I'm not saying this plan is bad, I'm saying the time is too tight. | 72 | 88 / 301 |
| 30 | negation | 没有人告诉我今天要交报告。 | No one told me that I had to hand in the report today. | No one told me the report was due today. | 71 | 89 / 197 |
| 31 | negation | 他并没有拒绝，只是想再考虑一下。 | He didn't refuse. He just wanted to think about it again. | He didn't refuse, he just wants to think about it more. | 68 | 104 / 254 |
| 32 | negation | 我们不能接受这个价格，但是可以再谈谈。 | We can't accept the price, but we can talk about it again. | We can't accept this price, but we can talk more. | 78 | 92 / 245 |
| 33 | negation | 我从来没有说过那样的话。 | I have never said that. | I never said such things. | 70 | 92 / 159 |
| 34 | negation | 别担心，这不是你的错。 | Don't worry, it's not your fault. | Don't worry, it's not your fault. | 75 | 89 / 219 |
| 35 | business words | 我们需要在周五之前把这个版本上线。 | We need to launch this version before Friday. | We need to launch this version before Friday. | 72 | 90 / 186 |
| 36 | business words | 请把这份合同走一下流程。 | Please follow the process of this contract. | Please process this contract through the workflow. | 70 | 89 / 178 |
| 37 | business words | 这个需求的优先级比较低，先排期吧。 | The priority of this demand is relatively low. Let's schedule it first. | This requirement has a relatively low priority, let's schedule it first. | 79 | 89 / 255 |
| 38 | business words | 我们要对齐一下口径，再跟客户沟通。 | We need to align the caliber and communicate with the customer. | We need to align our understanding before communicating with the client. | 91 | 89 / 221 |
| 39 | business words | 这笔款项已经到账了，请确认。 | This amount has been credited, please confirm. | This payment has arrived, please confirm. | 84 | 88 / 176 |
| 40 | business words | 老板觉得这个方案还需要打磨。 | The boss thinks that this plan still needs to be polished. | The boss thinks this plan still needs polishing. | 79 | 97 / 182 |
| 41 | business words | 我们的对手最近在降价抢市场。 | Our rivals have been grabbing the market with price reductions recently. | Our competitors have been lowering prices recently to grab market share. | 83 | 84 / 212 |
| 42 | business words | 开会之前请先看一下这个包，再看第六页的表格。 | Please take a look at this package before the meeting, and then look at the table on the sixth page. | Before the meeting, please take a look at this package, and then look at the table on page six. | 94 | 85 / 314 |
| 43 | incomplete | 我们先从上周的数据开始，然后 | Let's start with last week's data, and then | Let's start with last week's data, and then | 75 | 83 / 221 |
| 44 | incomplete | 因为客户那边还没有确认，所以我们可能 | Because the customer hasn't confirmed it yet, we may | Because the client hasn't confirmed yet, so we might | 70 | 84 / 210 |
| 45 | incomplete | 如果下周三还不行的话 | If next Wednesday doesn't work | If next Wednesday doesn't work then | 73 | 83 / 167 |
| 46 | incomplete | 我想问一下关于那个 | I want to ask about that. | I want to ask about that | 64 | 82 / 144 |
| 47 | incomplete | 就是说我们这边的情况是 | That is to say, the situation on our side is | So, the situation on our end is | 66 | 81 / 162 |
| 48 | incomplete | 所以最后的结论大概是 | So the final conclusion is probably | So the final conclusion is probably | 60 | 80 / 142 |

Translation time per sentence, 48 sentences, warm (median / p95 ms): system 72 / 99, language model first token 88 / 111, language model complete 230 / 314.

#### English to Chinese (the incoming direction)

| # | kind | source | system translation | language model | system ms | model first / total ms |
|---|---|---|---|---|---|---|
| 1 | conversational | Hello, can you hear me? | 你好，你能听到我说话吗？ | 你好，你能听到我吗？ | 60 | 27 / 96 |
| 2 | conversational | Sorry, the connection dropped for a second. Could you say that again? | 抱歉，连接中断了一秒钟。 你能再说一遍吗？ | 抱歉，连接断了一小会儿。您能再说一遍吗？ | 68 | 78 / 219 |
| 3 | conversational | Let's start with last week's numbers and then look at this week's progress. | 让我们从上周的数字开始，然后看看本周的进展情况。 | 我们先从上周的数据开始，然后再看看这周的进展。 | 66 | 78 / 221 |
| 4 | conversational | Are you free this weekend? We could grab dinner. | 你这个周末有空吗？ 我们可以去吃晚饭。 | 你这周末有空吗？我们可以一起吃个晚饭。 | 63 | 78 / 211 |
| 5 | conversational | Sure, no problem, I'll send it over right now. | 当然，没问题，我现在就把它寄过去。 | 好的，没问题，我现在就发过去。 | 63 | 79 / 175 |
| 6 | conversational | What do you think of the plan? Personally I think it's pretty good. | 你觉得这个计划怎么样？ 我个人认为它很不错。 | 你觉得这个计划怎么样？我个人觉得挺好的。 | 67 | 79 / 194 |
| 7 | conversational | Thanks for explaining that so patiently, I get it now. | 谢谢你如此耐心地解释，我现在明白了。 | 谢谢你这么耐心地解释，我明白了。 | 63 | 80 / 176 |
| 8 | conversational | My signal is bad here, so I might call you back from somewhere else. | 我这里的信号不好，所以我可能会从别的地方给你回电话。 | 我这儿信号不好，所以我可能从别的地方给你回电话。 | 70 | 80 / 226 |
| 9 | numbers | The budget is 3.5 million, not 5.3 million. | 预算是350万，而不是530万。 | 预算是350万，不是530万。 | 59 | 82 / 210 |
| 10 | numbers | Twelve people are coming, and seven of them are new colleagues. | 有十二个人要来，其中七人是新同事。 | 十二个人要来，其中有七个是新同事。 | 66 | 82 / 210 |
| 11 | numbers | The quote is 2,480 dollars per unit with a minimum order of 100. | 报价为每单位2480美元，最低订单100件。 | 报价是每单位2,480美元，最低订购量是100个。 | 71 | 81 / 286 |
| 12 | numbers | Growth was 15.3 percent, two points higher than last year. | 增长率为15.3%，比去年高出两个百分点。 | 增长了15.3%，比去年高了两个百分点。 | 69 | 83 / 232 |
| 13 | numbers | Please write down the number 415 555 0132. | 请写下号码415 555 0132。 | 请写下电话号码 415 555 0132。 | 66 | 86 / 289 |
| 14 | numbers | The total contract value is about twelve million dollars, paid in three instalments. | 合同总价值约为1200万美元，分三期支付。 | 总合同价值大约是1200万美元，分三期支付。 | 87 | 90 / 263 |
| 15 | numbers | The meeting room is on the third floor, room 308. | 会议室在三楼，308室。 | 会议室在三楼，308室。 | 87 | 91 / 212 |
| 16 | dates | Let's move the meeting to next Wednesday at 3:15 in the afternoon. | 让我们把会议改到下周三下午3:15。 | 我们把会议改到下周三下午3点15分吧。 | 97 | 90 / 265 |
| 17 | dates | I'll be in Shanghai on March 3rd and back in Beijing on the 4th in the afternoon. | 我将于3月3日在上海，4日下午回到北京。 | 我会在3月3号在上海，然后在4号下午回到北京。 | 104 | 89 / 262 |
| 18 | dates | Please submit the report by November 30th. | 请在11月30日之前提交报告。 | 请在11月30日之前提交报告。 | 88 | 90 / 223 |
| 19 | dates | We start at three fifteen. | 我们三点一刻开始。 | 我们三点十五开始。 | 82 | 87 / 155 |
| 20 | dates | We need roughly two to three weeks to finish testing. | 我们大约需要两到三周的时间来完成测试。 | 我们需要大概两到三周来完成测试。 | 100 | 88 / 200 |
| 21 | dates | Shipping is at the end of next month, the fifth at the latest. | 发货时间是下月底，最迟是5号。 | 发货在下个月底，最晚是5号。 | 113 | 87 / 232 |
| 22 | dates | The flight is at nine thirty, so be at the airport two hours early. | 航班是九点半，所以要提前两个小时到达机场。 | 航班是九点半，所以提前两个小时到机场。 | 116 | 85 / 229 |
| 23 | names | John Smith said he'll visit you tomorrow together with Li Ming. | John Smith说他明天会和Li Ming一起拜访你。 | John Smith 说他明天会和李明一起去看你。 | 112 | 85 / 228 |
| 24 | names | I'm David Chen and I work in procurement at Huawei. | 我是David Chen，在华为采购部门工作。 | 我是David Chen，我在华为做采购的。 | 102 | 84 / 192 |
| 25 | names | Please tell Ms. Garcia that the client is already waiting in the lobby. | 请告诉加西亚女士，客户已经在大厅里等着了。 | 请告诉加西亚女士，客户已经在大厅等着了。 | 100 | 83 / 219 |
| 26 | names | This is Sarah from Microsoft, she's in charge of cloud services. | 这是微软的Sarah，她负责云服务。 | 我是来自微软的Sarah，负责云服务。 | 104 | 82 / 188 |
| 27 | names | We're going to Munich next month for the Hannover Messe trade fair. | 我们下个月要去慕尼黑参加汉诺威博览会。 | 我们下个月要去慕尼黑参加汉诺威工业博览会。 | 98 | 80 / 246 |
| 28 | names | Professor Zhou's paper was published in Nature. | 周教授的论文发表在《自然》杂志上。 | 周教授的论文发表在Nature上了。 | 86 | 79 / 172 |
| 29 | negation | I'm not saying the plan won't work, I'm saying the timing is too tight. | 我不是说这个计划行不通，我是说时机太紧了。 | 我不是说这个计划不会成功，我是说时间太紧了。 | 99 | 79 / 231 |
| 30 | negation | Nobody told me the report was due today. | 没有人告诉我报告是今天到期的。 | 没人告诉我报告是今天到期的。 | 73 | 79 / 162 |
| 31 | negation | He didn't say no, he just wants to think about it. | 他没有说不，他只是想考虑一下。 | 他没有说不，他只是想考虑一下。 | 90 | 81 / 194 |
| 32 | negation | We can't accept this price, but we can talk about it. | 我们不能接受这个价格，但我们可以谈谈。 | 我们不能接受这个价格，但我们可以谈谈。 | 87 | 81 / 194 |
| 33 | negation | I never said anything like that. | 我从未说过那种话。 | 我从来没有说过那样的话。 | 77 | 80 / 153 |
| 34 | negation | Don't worry, it's not your fault. | 别担心，这不是你的错。 | 别担心，这不是你的错。 | 82 | 81 / 157 |
| 35 | business words | We need to ship this build by Friday. | 我们需要在周五之前发货。 | 我们需要在周五前发货这个构建。 | 83 | 80 / 185 |
| 36 | business words | Can you run this contract through the process? | 你能完成这个合同吗？ | 你能把这份合同跑一遍流程吗？ | 83 | 82 / 176 |
| 37 | business words | The priority of this request is low, let's put it in the backlog. | 这个请求的优先级很低，让我们把它放在积压中。 | 这个请求的优先级很低，我们把它放进积压列表吧。 | 87 | 80 / 240 |
| 38 | business words | We should align on the messaging before we talk to the client. | 在与客户交谈之前，我们应该在信息上保持一致。 | 在我们和客户谈之前，我们应该就信息达成一致。 | 79 | 81 / 210 |
| 39 | business words | The payment has cleared, please confirm. | 付款已结清，请确认。 | 付款已经到账了，请确认一下。 | 70 | 80 / 180 |
| 40 | business words | The boss thinks the proposal still needs polishing. | 老板认为提案仍然需要打磨。 | 老板觉得这个提案还需要润色一下。 | 72 | 79 / 170 |
| 41 | business words | Our competitor has been cutting prices to grab the market. | 我们的竞争对手一直在降价以抢夺市场。 | 我们的竞争对手一直在降价来抢占市场。 | 71 | 79 / 188 |
| 42 | business words | Please check the deck before the call, and mind the table on page six. | 请在通话前检查甲板，并注意第六页的表格。 | 请在电话前检查一下演示文稿，注意第六页的表格。 | 76 | 79 / 237 |
| 43 | incomplete | Let's start with last week's numbers and then | 让我们从上周的数字开始，然后 | 我们从上周的数据开始，然后 | 66 | 79 / 157 |
| 44 | incomplete | Because the client hasn't confirmed yet, we might | 因为客户还没有确认，我们可能会 | 因为客户还没确认，我们可能 | 64 | 81 / 154 |
| 45 | incomplete | If it still doesn't work next Wednesday | 如果下周三还是不行 | 如果下周三还是不行 | 70 | 81 / 146 |
| 46 | incomplete | I wanted to ask about that | 我想问一下那个 | 我想问一下那个 | 72 | 81 / 125 |
| 47 | incomplete | So what's happening on our side is | 所以，我们这边正在发生的事情是 | 所以我们这边的情况是 | 75 | 81 / 135 |
| 48 | incomplete | So the bottom line is probably | 所以底线可能是 | 所以底线大概是 | 74 | 81 / 135 |

Translation time per sentence, 48 sentences, warm (median / p95 ms): system 77 / 112, language model first token 81 / 90, language model complete 200 / 265.


## 3. Reading the quality sample

My own reading of the 96 pairs, by a reader of both languages; it is not a scored evaluation.

- **Numbers, dates, names, negation: both are good and about equal.** Neither turned a negation round. The system
  engine wrote "5.30 million" for 五百三十万 (the model: "5.3 million"), "2480" without a separator in Chinese
  (fine), and kept the English names Li Ming and John Smith unconverted in Chinese where the model wrote 李明 in one
  of them; the model wrote "Wang Jianguao" for 王建国 (a transliteration slip) where the system engine wrote
  "Wang Jianguo". Spoken digit strings: the model produced "138-0013-8000", the system engine "138, 0013, 8000", and
  both are speakable.
- **Tense without context is the system engine's main weakness in this sample.** 我三月三号到上海，四号下午回北京
  became "I arrived in Shanghai on March 3rd and returned ..." (a plan told as a past event). The model said "I arrive ...
  and return ...".
- **Ambiguous business words: both stumble, differently.** 下午三点十五分开会 became "The meeting will be separated at
  3:15 p.m." (system) and "will be split" (model), both wrong; "deck" became 甲板 (the ship's deck) in the system engine
  and 演示文稿 in the model, which is right; "ship this build" became 发货 (system) and 发货这个构建 (model), both
  clumsy; "run this contract through the process" became 你能完成这个合同吗？ (system: the meaning is lost) against
  你能把这份合同跑一遍流程吗？ (model: right). 对齐一下口径 became "align the caliber" (system, a literal reading)
  against "align our understanding" (model). The model reads business register better; the system engine is more
  literal and sometimes wrong in a way a listener cannot spot.
- **Incomplete clauses: both cope.** Neither invented an ending; the system engine sometimes added a full stop
  ("I want to ask about that.") that the speech did not have. Wording differs in small ways: one Chinese
  clause ("...所以我们可能" became "...so we might" in the model, "we may" in the system engine), and the model
  added "then" once ("If next Wednesday doesn't work then").
- **Style.** The system engine is closer to the words ("a total of twelve people participated", "the quotation is");
  the model is more like speech ("grab a meal", "we're going to Munich"). Both are fully usable for a conversation.
- **Time.** System translation: median 72 ms (zh to en) and 77 ms (en to zh) per sentence, warm, p95 99 and 112 ms.
  Model: first token median 88 and 81 ms, complete sentence 230 and 200 ms (p95 314 and 265 ms). Both are well inside
  the time a clause takes to be recognised and spoken.

Conclusion: system translation is a good default for speed, memory (nothing to load) and robustness, and its text is
as good as the model's on the sentence kinds that matter most (numbers, dates, names, negation). It is weaker on
ambiguous words and on tense without context, which is why the model stays available as the alternative, and the
settings say so in one line.

