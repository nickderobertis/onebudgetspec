"""Evaluate the subset of GitHub Actions expressions this repository's workflows use.

A job-level ``if:`` cannot be run outside a hosted runner, so the guard test evaluates it
here instead: literals, context lookups (``vars.X``, ``needs.guard.outputs.enabled``,
``github.event_name``), ``==``, ``!=``, ``!``, ``&&``, ``||``, parentheses and the status
functions. Comparison follows GitHub's rules for what these workflows compare: strings
compare case-insensitively, and a missing value equals only the empty string.
"""

import re
from collections.abc import Mapping
from dataclasses import dataclass
from typing import Literal

Value = str | bool | None

_TOKEN = re.compile(
    r"\s*(?:(?P<string>'(?:[^']|'')*')|(?P<op>==|!=|&&|\|\||!|\(|\))"
    r"|(?P<name>[A-Za-z_][A-Za-z0-9_\-]*(?:\.[A-Za-z_*][A-Za-z0-9_\-]*)*))"
)


@dataclass(frozen=True)
class Token:
    """One lexical unit of an expression."""

    kind: Literal["string", "op", "name"]
    text: str


class ExpressionError(ValueError):
    """The expression uses syntax this evaluator does not model."""


@dataclass(frozen=True)
class Status:
    """What the status functions answer: whether the needed jobs all succeeded."""

    success: bool = True


def _tokens(expression: str) -> list[Token]:
    text = expression.strip()
    if text.startswith("${{") and text.endswith("}}"):
        text = text[3:-2]
    tokens: list[Token] = []
    position = 0
    while position < len(text):
        if text[position:].strip() == "":
            break
        match = _TOKEN.match(text, position)
        if match is None:
            raise ExpressionError(f"cannot read {text[position:]!r} in {expression!r}")
        match match.lastgroup:
            case "string" | "op" | "name" as kind:
                tokens.append(Token(kind, match.group(kind)))
            case _:  # pragma: no cover - the pattern has exactly these three groups
                raise ExpressionError(f"cannot read {text[position:]!r} in {expression!r}")
        position = match.end()
    return tokens


def _truthy(value: Value) -> bool:
    return bool(value)


def _equal(left: Value, right: Value) -> bool:
    if isinstance(left, bool) or isinstance(right, bool):
        return left == right
    return (left or "").casefold() == (right or "").casefold()


class _Parser:
    def __init__(self, expression: str, context: Mapping[str, Value], status: Status) -> None:
        self.expression = expression
        self.tokens = _tokens(expression)
        self.position = 0
        self.context = context
        self.status = status

    def parse(self) -> Value:
        value = self.or_()
        if self.position != len(self.tokens):
            raise ExpressionError(
                f"unexpected {self.tokens[self.position].text!r} in {self.expression!r}"
            )
        return value

    def peek(self) -> str | None:
        return self.tokens[self.position].text if self.position < len(self.tokens) else None

    def take(self) -> Token:
        if self.position >= len(self.tokens):
            raise ExpressionError(f"{self.expression!r} ends early")
        token = self.tokens[self.position]
        self.position += 1
        return token

    def or_(self) -> Value:
        value = self.and_()
        while self.peek() == "||":
            self.take()
            right = self.and_()
            value = value if _truthy(value) else right
        return value

    def and_(self) -> Value:
        value = self.comparison()
        while self.peek() == "&&":
            self.take()
            right = self.comparison()
            value = right if _truthy(value) else value
        return value

    def comparison(self) -> Value:
        value = self.unary()
        while self.peek() in ("==", "!="):
            operator = self.take().text
            right = self.unary()
            equal = _equal(value, right)
            value = equal if operator == "==" else not equal
        return value

    def unary(self) -> Value:
        if self.peek() == "!":
            self.take()
            return not _truthy(self.unary())
        return self.primary()

    def primary(self) -> Value:
        token = self.take()
        match token:
            case Token("string", text):
                return text[1:-1].replace("''", "'")
            case Token("op", "("):
                value = self.or_()
                if self.take().text != ")":
                    raise ExpressionError(f"unbalanced parentheses in {self.expression!r}")
                return value
            case Token("name", text) if self.peek() == "(":
                return self.call(text)
            case Token("name", "true" | "false" as text):
                return text == "true"
            case Token("name", text):
                return self.context.get(text)
            case _:
                raise ExpressionError(f"unexpected {token.text!r} in {self.expression!r}")

    def call(self, name: str) -> Value:
        self.take()
        if self.take().text != ")":
            raise ExpressionError(f"{name}() takes no arguments here: {self.expression!r}")
        answers = {
            "always": True,
            "cancelled": False,
            "success": self.status.success,
            "failure": not self.status.success,
        }
        if name not in answers:
            raise ExpressionError(f"{name}() is not modelled: {self.expression!r}")
        return answers[name]


def uses_status_function(expression: str) -> bool:
    """Whether ``expression`` calls a status function, replacing the implicit ``success()``."""
    tokens = _tokens(expression)
    return any(
        token.kind == "name"
        and token.text in ("always", "cancelled", "success", "failure")
        and following.text == "("
        for token, following in zip(tokens, tokens[1:], strict=False)
    )


def evaluate(expression: str, context: Mapping[str, Value], status: Status | None = None) -> bool:
    """Whether a job or step with ``if: expression`` runs, given its context values.

    Args:
        expression: the ``if:`` text, with or without ``${{ }}``.
        context: dotted context names (``vars.NPM_PUBLISH``) to their values; a name
            absent here is unset.
        status: whether the jobs it needs succeeded; they did when omitted.

    Returns:
        Whether it runs. Without a status function, GitHub also requires success.
    """
    status = status or Status()
    value = _truthy(_Parser(expression, context, status).parse())
    if not uses_status_function(expression):
        value = value and status.success
    return value
