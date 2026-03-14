## Real Section

This section contains actual content that the parser should recognize. Below is a code block that contains markdown-like syntax which the parser must ignore.

```markdown
## Fake Heading

This looks like a heading but it is inside a fenced code block.
The parser should not treat this as a real section. #fake-tag
```

The parser should resume recognizing content here. This paragraph comes after the code block and belongs to the "Real Section" heading. Any tags or headings inside fenced code blocks are not part of the document structure.
