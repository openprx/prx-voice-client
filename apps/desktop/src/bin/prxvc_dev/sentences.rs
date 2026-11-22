//! The sentences of the engine comparison: conversational speech, numbers, dates and times, names, negation, ambiguous business words and incomplete clauses, 48 per direction. The Chinese text is written as escapes in this file only so that the source stays ASCII.

/// Chinese sentences, translated to English (the outgoing direction).
pub(crate) const ZH: &[(&str, &str)] = &[
    (
        "conversational",
        "\u{4f60}\u{597d}\u{ff0c}\u{80fd}\u{542c}\u{5230}\u{6211}\u{8bf4}\u{8bdd}\u{5417}\u{ff1f}",
    ),
    (
        "conversational",
        "\u{4e0d}\u{597d}\u{610f}\u{601d}\u{ff0c}\u{521a}\u{624d}\u{7f51}\u{7edc}\u{6709}\u{70b9}\u{5361}\u{ff0c}\u{4f60}\u{518d}\u{8bf4}\u{4e00}\u{904d}\u{597d}\u{5417}\u{ff1f}",
    ),
    (
        "conversational",
        "\u{6211}\u{4eec}\u{5148}\u{4ece}\u{4e0a}\u{5468}\u{7684}\u{6570}\u{636e}\u{5f00}\u{59cb}\u{ff0c}\u{7136}\u{540e}\u{518d}\u{770b}\u{8fd9}\u{5468}\u{7684}\u{8fdb}\u{5c55}\u{3002}",
    ),
    (
        "conversational",
        "\u{8fd9}\u{4e2a}\u{5468}\u{672b}\u{4f60}\u{6709}\u{7a7a}\u{5417}\u{ff1f}\u{6211}\u{4eec}\u{4e00}\u{8d77}\u{5403}\u{4e2a}\u{996d}\u{5427}\u{3002}",
    ),
    (
        "conversational",
        "\u{597d}\u{7684}\u{ff0c}\u{6ca1}\u{95ee}\u{9898}\u{ff0c}\u{6211}\u{9a6c}\u{4e0a}\u{53d1}\u{7ed9}\u{4f60}\u{3002}",
    ),
    (
        "conversational",
        "\u{4f60}\u{89c9}\u{5f97}\u{8fd9}\u{4e2a}\u{65b9}\u{6848}\u{600e}\u{4e48}\u{6837}\u{ff1f}\u{6211}\u{4e2a}\u{4eba}\u{89c9}\u{5f97}\u{8fd8}\u{4e0d}\u{9519}\u{3002}",
    ),
    (
        "conversational",
        "\u{8c22}\u{8c22}\u{4f60}\u{7684}\u{8010}\u{5fc3}\u{89e3}\u{91ca}\u{ff0c}\u{6211}\u{73b0}\u{5728}\u{660e}\u{767d}\u{4e86}\u{3002}",
    ),
    (
        "conversational",
        "\u{6211}\u{8fd9}\u{8fb9}\u{4fe1}\u{53f7}\u{4e0d}\u{592a}\u{597d}\u{ff0c}\u{53ef}\u{80fd}\u{5f97}\u{6362}\u{4e2a}\u{5730}\u{65b9}\u{518d}\u{6253}\u{7ed9}\u{4f60}\u{3002}",
    ),
    (
        "numbers",
        "\u{8fd9}\u{4e2a}\u{9879}\u{76ee}\u{7684}\u{9884}\u{7b97}\u{662f}\u{4e09}\u{767e}\u{4e94}\u{5341}\u{4e07}\u{ff0c}\u{4e0d}\u{662f}\u{4e94}\u{767e}\u{4e09}\u{5341}\u{4e07}\u{3002}",
    ),
    (
        "numbers",
        "\u{4e00}\u{5171}\u{6709}\u{5341}\u{4e8c}\u{4e2a}\u{4eba}\u{53c2}\u{52a0}\u{ff0c}\u{5176}\u{4e2d}\u{4e03}\u{4e2a}\u{662f}\u{65b0}\u{540c}\u{4e8b}\u{3002}",
    ),
    (
        "numbers",
        "\u{62a5}\u{4ef7}\u{662f}\u{6bcf}\u{53f0}\u{4e24}\u{5343}\u{56db}\u{767e}\u{516b}\u{5341}\u{5143}\u{ff0c}\u{4e00}\u{767e}\u{53f0}\u{8d77}\u{8ba2}\u{3002}",
    ),
    (
        "numbers",
        "\u{589e}\u{957f}\u{7387}\u{662f}\u{767e}\u{5206}\u{4e4b}\u{5341}\u{4e94}\u{70b9}\u{4e09}\u{ff0c}\u{6bd4}\u{53bb}\u{5e74}\u{9ad8}\u{4e86}\u{4e24}\u{4e2a}\u{767e}\u{5206}\u{70b9}\u{3002}",
    ),
    (
        "numbers",
        "\u{8bf7}\u{628a}\u{7535}\u{8bdd}\u{53f7}\u{7801}\u{4e00}\u{4e09}\u{516b}\u{ff0c}\u{96f6}\u{96f6}\u{4e00}\u{4e09}\u{ff0c}\u{516b}\u{96f6}\u{96f6}\u{96f6}\u{8bb0}\u{4e0b}\u{6765}\u{3002}",
    ),
    (
        "numbers",
        "\u{5408}\u{540c}\u{603b}\u{989d}\u{5927}\u{7ea6}\u{4e00}\u{5343}\u{4e8c}\u{767e}\u{4e07}\u{7f8e}\u{5143}\u{ff0c}\u{5206}\u{4e09}\u{671f}\u{652f}\u{4ed8}\u{3002}",
    ),
    (
        "numbers",
        "\u{4f1a}\u{8bae}\u{5ba4}\u{5728}\u{4e09}\u{697c}\u{ff0c}\u{623f}\u{95f4}\u{53f7}\u{662f}\u{4e09}\u{96f6}\u{516b}\u{3002}",
    ),
    (
        "dates",
        "\u{4f1a}\u{8bae}\u{6539}\u{5230}\u{4e0b}\u{5468}\u{4e09}\u{4e0b}\u{5348}\u{4e09}\u{70b9}\u{5341}\u{4e94}\u{5206}\u{3002}",
    ),
    (
        "dates",
        "\u{6211}\u{4e09}\u{6708}\u{4e09}\u{53f7}\u{5230}\u{4e0a}\u{6d77}\u{ff0c}\u{56db}\u{53f7}\u{4e0b}\u{5348}\u{56de}\u{5317}\u{4eac}\u{3002}",
    ),
    (
        "dates",
        "\u{8bf7}\u{5728}\u{5341}\u{4e00}\u{6708}\u{4e09}\u{5341}\u{53f7}\u{4e4b}\u{524d}\u{63d0}\u{4ea4}\u{62a5}\u{544a}\u{3002}",
    ),
    (
        "dates",
        "\u{4e0b}\u{5348}\u{4e09}\u{70b9}\u{5341}\u{4e94}\u{5206}\u{5f00}\u{4f1a}\u{3002}",
    ),
    (
        "dates",
        "\u{6211}\u{4eec}\u{5927}\u{6982}\u{9700}\u{8981}\u{4e24}\u{5230}\u{4e09}\u{4e2a}\u{661f}\u{671f}\u{6765}\u{5b8c}\u{6210}\u{6d4b}\u{8bd5}\u{3002}",
    ),
    (
        "dates",
        "\u{53d1}\u{8d27}\u{65f6}\u{95f4}\u{662f}\u{4e0b}\u{4e2a}\u{6708}\u{6708}\u{5e95}\u{ff0c}\u{6700}\u{665a}\u{4e0d}\u{8d85}\u{8fc7}\u{4e94}\u{53f7}\u{3002}",
    ),
    (
        "dates",
        "\u{65e9}\u{4e0a}\u{4e5d}\u{70b9}\u{534a}\u{7684}\u{822a}\u{73ed}\u{ff0c}\u{4f60}\u{5f97}\u{63d0}\u{524d}\u{4e24}\u{4e2a}\u{5c0f}\u{65f6}\u{5230}\u{673a}\u{573a}\u{3002}",
    ),
    (
        "names",
        "\u{738b}\u{5efa}\u{56fd}\u{8bf4}\u{4ed6}\u{660e}\u{5929}\u{4f1a}\u{548c}\u{674e}\u{660e}\u{4e00}\u{8d77}\u{6765}\u{62dc}\u{8bbf}\u{4f60}\u{4eec}\u{3002}",
    ),
    (
        "names",
        "\u{6211}\u{662f}\u{5f20}\u{4f1f}\u{ff0c}\u{5728}\u{534e}\u{4e3a}\u{7684}\u{91c7}\u{8d2d}\u{90e8}\u{5de5}\u{4f5c}\u{3002}",
    ),
    (
        "names",
        "\u{8bf7}\u{8f6c}\u{544a}\u{9648}\u{7ecf}\u{7406}\u{ff0c}\u{5ba2}\u{6237}\u{5df2}\u{7ecf}\u{5728}\u{5927}\u{5385}\u{7b49}\u{4ed6}\u{4e86}\u{3002}",
    ),
    (
        "names",
        "\u{8fd9}\u{4f4d}\u{662f}\u{6765}\u{81ea}\u{5fae}\u{8f6f}\u{7684}Sarah\u{ff0c}\u{5979}\u{8d1f}\u{8d23}\u{4e91}\u{670d}\u{52a1}\u{3002}",
    ),
    (
        "names",
        "\u{6211}\u{4eec}\u{4e0b}\u{4e2a}\u{6708}\u{8981}\u{53bb}\u{6155}\u{5c3c}\u{9ed1}\u{53c2}\u{52a0}\u{6c49}\u{8bfa}\u{5a01}\u{5de5}\u{4e1a}\u{5c55}\u{3002}",
    ),
    (
        "names",
        "\u{5468}\u{6559}\u{6388}\u{7684}\u{8bba}\u{6587}\u{53d1}\u{8868}\u{5728}\u{300a}\u{81ea}\u{7136}\u{300b}\u{6742}\u{5fd7}\u{4e0a}\u{3002}",
    ),
    (
        "negation",
        "\u{6211}\u{4e0d}\u{662f}\u{8bf4}\u{8fd9}\u{4e2a}\u{65b9}\u{6848}\u{4e0d}\u{884c}\u{ff0c}\u{6211}\u{662f}\u{8bf4}\u{65f6}\u{95f4}\u{592a}\u{7d27}\u{4e86}\u{3002}",
    ),
    (
        "negation",
        "\u{6ca1}\u{6709}\u{4eba}\u{544a}\u{8bc9}\u{6211}\u{4eca}\u{5929}\u{8981}\u{4ea4}\u{62a5}\u{544a}\u{3002}",
    ),
    (
        "negation",
        "\u{4ed6}\u{5e76}\u{6ca1}\u{6709}\u{62d2}\u{7edd}\u{ff0c}\u{53ea}\u{662f}\u{60f3}\u{518d}\u{8003}\u{8651}\u{4e00}\u{4e0b}\u{3002}",
    ),
    (
        "negation",
        "\u{6211}\u{4eec}\u{4e0d}\u{80fd}\u{63a5}\u{53d7}\u{8fd9}\u{4e2a}\u{4ef7}\u{683c}\u{ff0c}\u{4f46}\u{662f}\u{53ef}\u{4ee5}\u{518d}\u{8c08}\u{8c08}\u{3002}",
    ),
    (
        "negation",
        "\u{6211}\u{4ece}\u{6765}\u{6ca1}\u{6709}\u{8bf4}\u{8fc7}\u{90a3}\u{6837}\u{7684}\u{8bdd}\u{3002}",
    ),
    (
        "negation",
        "\u{522b}\u{62c5}\u{5fc3}\u{ff0c}\u{8fd9}\u{4e0d}\u{662f}\u{4f60}\u{7684}\u{9519}\u{3002}",
    ),
    (
        "business words",
        "\u{6211}\u{4eec}\u{9700}\u{8981}\u{5728}\u{5468}\u{4e94}\u{4e4b}\u{524d}\u{628a}\u{8fd9}\u{4e2a}\u{7248}\u{672c}\u{4e0a}\u{7ebf}\u{3002}",
    ),
    (
        "business words",
        "\u{8bf7}\u{628a}\u{8fd9}\u{4efd}\u{5408}\u{540c}\u{8d70}\u{4e00}\u{4e0b}\u{6d41}\u{7a0b}\u{3002}",
    ),
    (
        "business words",
        "\u{8fd9}\u{4e2a}\u{9700}\u{6c42}\u{7684}\u{4f18}\u{5148}\u{7ea7}\u{6bd4}\u{8f83}\u{4f4e}\u{ff0c}\u{5148}\u{6392}\u{671f}\u{5427}\u{3002}",
    ),
    (
        "business words",
        "\u{6211}\u{4eec}\u{8981}\u{5bf9}\u{9f50}\u{4e00}\u{4e0b}\u{53e3}\u{5f84}\u{ff0c}\u{518d}\u{8ddf}\u{5ba2}\u{6237}\u{6c9f}\u{901a}\u{3002}",
    ),
    (
        "business words",
        "\u{8fd9}\u{7b14}\u{6b3e}\u{9879}\u{5df2}\u{7ecf}\u{5230}\u{8d26}\u{4e86}\u{ff0c}\u{8bf7}\u{786e}\u{8ba4}\u{3002}",
    ),
    (
        "business words",
        "\u{8001}\u{677f}\u{89c9}\u{5f97}\u{8fd9}\u{4e2a}\u{65b9}\u{6848}\u{8fd8}\u{9700}\u{8981}\u{6253}\u{78e8}\u{3002}",
    ),
    (
        "business words",
        "\u{6211}\u{4eec}\u{7684}\u{5bf9}\u{624b}\u{6700}\u{8fd1}\u{5728}\u{964d}\u{4ef7}\u{62a2}\u{5e02}\u{573a}\u{3002}",
    ),
    (
        "business words",
        "\u{5f00}\u{4f1a}\u{4e4b}\u{524d}\u{8bf7}\u{5148}\u{770b}\u{4e00}\u{4e0b}\u{8fd9}\u{4e2a}\u{5305}\u{ff0c}\u{518d}\u{770b}\u{7b2c}\u{516d}\u{9875}\u{7684}\u{8868}\u{683c}\u{3002}",
    ),
    (
        "incomplete",
        "\u{6211}\u{4eec}\u{5148}\u{4ece}\u{4e0a}\u{5468}\u{7684}\u{6570}\u{636e}\u{5f00}\u{59cb}\u{ff0c}\u{7136}\u{540e}",
    ),
    (
        "incomplete",
        "\u{56e0}\u{4e3a}\u{5ba2}\u{6237}\u{90a3}\u{8fb9}\u{8fd8}\u{6ca1}\u{6709}\u{786e}\u{8ba4}\u{ff0c}\u{6240}\u{4ee5}\u{6211}\u{4eec}\u{53ef}\u{80fd}",
    ),
    (
        "incomplete",
        "\u{5982}\u{679c}\u{4e0b}\u{5468}\u{4e09}\u{8fd8}\u{4e0d}\u{884c}\u{7684}\u{8bdd}",
    ),
    (
        "incomplete",
        "\u{6211}\u{60f3}\u{95ee}\u{4e00}\u{4e0b}\u{5173}\u{4e8e}\u{90a3}\u{4e2a}",
    ),
    (
        "incomplete",
        "\u{5c31}\u{662f}\u{8bf4}\u{6211}\u{4eec}\u{8fd9}\u{8fb9}\u{7684}\u{60c5}\u{51b5}\u{662f}",
    ),
    (
        "incomplete",
        "\u{6240}\u{4ee5}\u{6700}\u{540e}\u{7684}\u{7ed3}\u{8bba}\u{5927}\u{6982}\u{662f}",
    ),
];

/// English sentences, translated to Chinese (the incoming direction).
pub(crate) const EN: &[(&str, &str)] = &[
    ("conversational", "Hello, can you hear me?"),
    (
        "conversational",
        "Sorry, the connection dropped for a second. Could you say that again?",
    ),
    (
        "conversational",
        "Let's start with last week's numbers and then look at this week's progress.",
    ),
    (
        "conversational",
        "Are you free this weekend? We could grab dinner.",
    ),
    (
        "conversational",
        "Sure, no problem, I'll send it over right now.",
    ),
    (
        "conversational",
        "What do you think of the plan? Personally I think it's pretty good.",
    ),
    (
        "conversational",
        "Thanks for explaining that so patiently, I get it now.",
    ),
    (
        "conversational",
        "My signal is bad here, so I might call you back from somewhere else.",
    ),
    ("numbers", "The budget is 3.5 million, not 5.3 million."),
    (
        "numbers",
        "Twelve people are coming, and seven of them are new colleagues.",
    ),
    (
        "numbers",
        "The quote is 2,480 dollars per unit with a minimum order of 100.",
    ),
    (
        "numbers",
        "Growth was 15.3 percent, two points higher than last year.",
    ),
    ("numbers", "Please write down the number 415 555 0132."),
    (
        "numbers",
        "The total contract value is about twelve million dollars, paid in three instalments.",
    ),
    (
        "numbers",
        "The meeting room is on the third floor, room 308.",
    ),
    (
        "dates",
        "Let's move the meeting to next Wednesday at 3:15 in the afternoon.",
    ),
    (
        "dates",
        "I'll be in Shanghai on March 3rd and back in Beijing on the 4th in the afternoon.",
    ),
    ("dates", "Please submit the report by November 30th."),
    ("dates", "We start at three fifteen."),
    (
        "dates",
        "We need roughly two to three weeks to finish testing.",
    ),
    (
        "dates",
        "Shipping is at the end of next month, the fifth at the latest.",
    ),
    (
        "dates",
        "The flight is at nine thirty, so be at the airport two hours early.",
    ),
    (
        "names",
        "John Smith said he'll visit you tomorrow together with Li Ming.",
    ),
    (
        "names",
        "I'm David Chen and I work in procurement at Huawei.",
    ),
    (
        "names",
        "Please tell Ms. Garcia that the client is already waiting in the lobby.",
    ),
    (
        "names",
        "This is Sarah from Microsoft, she's in charge of cloud services.",
    ),
    (
        "names",
        "We're going to Munich next month for the Hannover Messe trade fair.",
    ),
    ("names", "Professor Zhou's paper was published in Nature."),
    (
        "negation",
        "I'm not saying the plan won't work, I'm saying the timing is too tight.",
    ),
    ("negation", "Nobody told me the report was due today."),
    (
        "negation",
        "He didn't say no, he just wants to think about it.",
    ),
    (
        "negation",
        "We can't accept this price, but we can talk about it.",
    ),
    ("negation", "I never said anything like that."),
    ("negation", "Don't worry, it's not your fault."),
    ("business words", "We need to ship this build by Friday."),
    (
        "business words",
        "Can you run this contract through the process?",
    ),
    (
        "business words",
        "The priority of this request is low, let's put it in the backlog.",
    ),
    (
        "business words",
        "We should align on the messaging before we talk to the client.",
    ),
    ("business words", "The payment has cleared, please confirm."),
    (
        "business words",
        "The boss thinks the proposal still needs polishing.",
    ),
    (
        "business words",
        "Our competitor has been cutting prices to grab the market.",
    ),
    (
        "business words",
        "Please check the deck before the call, and mind the table on page six.",
    ),
    (
        "incomplete",
        "Let's start with last week's numbers and then",
    ),
    (
        "incomplete",
        "Because the client hasn't confirmed yet, we might",
    ),
    ("incomplete", "If it still doesn't work next Wednesday"),
    ("incomplete", "I wanted to ask about that"),
    ("incomplete", "So what's happening on our side is"),
    ("incomplete", "So the bottom line is probably"),
];
