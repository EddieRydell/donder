import type {
  SequenceGraphOperator,
  SequenceGraphOperatorDefinition
} from "../../../types";

export function graphOperatorDefinition(
  catalog: SequenceGraphOperatorDefinition[],
  operator: SequenceGraphOperator
) {
  const key = graphOperatorKey(operator);
  const definition = catalog.find((candidate) => graphOperatorKey(candidate.operator) === key);
  if (definition === undefined) {
    throw new Error(`Missing graph operator catalog entry for ${key}`);
  }
  return definition;
}

export function graphOperatorKey(operator: SequenceGraphOperator) {
  return `custom:${operator.moduleId}:${operator.path}:${operator.objectKey}`;
}
