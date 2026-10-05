"""The expression evaluator answers as GitHub does for the forms the workflows use."""

import pytest

from repo_checks.expressions import ExpressionError, Status, evaluate, uses_status_function


@pytest.mark.parametrize(
    ("expression", "context", "expected"),
    [
        ("${{ vars.NPM_PUBLISH == 'true' }}", {"vars.NPM_PUBLISH": "true"}, True),
        ("vars.NPM_PUBLISH == 'true'", {"vars.NPM_PUBLISH": "TRUE"}, True),
        ("vars.NPM_PUBLISH == 'true'", {"vars.NPM_PUBLISH": "false"}, False),
        ("vars.NPM_PUBLISH == 'true'", {}, False),
        ("vars.NPM_PUBLISH != 'true'", {}, True),
        ("vars.MISSING == ''", {}, True),
        (
            "github.event_name == 'pull_request' && x != 'edited'",
            {"github.event_name": "pull_request", "x": "opened"},
            True,
        ),
        (
            "github.event_name == 'push' || x == 'y'",
            {"github.event_name": "pull_request", "x": "y"},
            True,
        ),
        ("github.event_name == 'push' || x == 'y'", {"github.event_name": "pull_request"}, False),
        ("!(a == 'b')", {"a": "b"}, False),
        ("!a", {}, True),
        ("a", {"a": "set"}, True),
        ("true && false", {}, False),
        ("true == true", {}, True),
        ("'it''s' == 'IT''S'", {}, True),
        ("always()", {}, True),
    ],
)
def test_evaluates(expression: str, context: dict, expected: bool) -> None:
    assert evaluate(expression, context) is expected


def test_without_a_status_function_failed_needs_skip_the_job() -> None:
    assert evaluate("a == 'b'", {"a": "b"}, Status(success=False)) is False
    assert evaluate("always() && a == 'b'", {"a": "b"}, Status(success=False)) is True
    assert evaluate("failure()", {}, Status(success=False)) is True
    assert evaluate("success()", {}, Status(success=False)) is False
    assert evaluate("!cancelled()", {}) is True
    assert uses_status_function("${{ always() }}")
    assert not uses_status_function("a == 'b'")
    assert not uses_status_function("success == 'yes'")
    assert evaluate("success == 'yes'", {"success": "yes"}, Status(success=False)) is False


@pytest.mark.parametrize(
    "expression",
    ["a ==", "a == 'b' )", "(a == 'b'", "contains(a)", "always(x)", "a ~ b", "== a"],
)
def test_refuses_what_it_does_not_model(expression: str) -> None:
    with pytest.raises(ExpressionError):
        evaluate(expression, {"a": "b"})
