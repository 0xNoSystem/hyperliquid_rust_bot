import ace from "ace-builds";
// Import the base components needed for inheritance
import "ace-builds/src-noconflict/mode-text";
import "ace-builds/src-noconflict/mode-java";

// Use the internal ace.require to get the base classes
const TextHighlightRules = ace.require(
    "ace/mode/text_highlight_rules"
).TextHighlightRules;
const JavaMode = ace.require("ace/mode/java").Mode;

export class CustomHighlightRules extends TextHighlightRules {
    constructor() {
        super();
        this.$rules = {
            start: [
                { token: "comment", regex: "#.*$" },
                { token: "string", regex: '".*?"' },
                {
                    token: "keyword",
                    regex: "\\b(let|if|else|return|for|while|fn|struct|enum)\\b",
                },
                {
                    token: ["punctuation.operator", "variable.field"],
                    regex: "(\\.)(value|on_close|ts)\\b",
                },

                { token: "keyword2", regex: "\\b(extract|timedelta)\\b" },

                {
                    token: "constant.language.boolean",
                    regex: "\\b(true|false)\\b",
                },

                { token: "constant.numeric", regex: "\\b\\d+(\\.\\d+)?\\b" },

                { token: "paren", regex: "[\\[\\(\\{\\}\\]\\)]" },
            ],
        };
    }
}

export default class CustomRhaiMode extends JavaMode {
    constructor() {
        super();
        this.HighlightRules = CustomHighlightRules;
    }
}
