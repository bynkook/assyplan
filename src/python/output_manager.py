"""Output manager module for saving analysis results to files.

This module provides centralized functions to export all analysis outputs
to the project's output folder for user review.

Output files saved to: {project_root}/output/
- node_table.csv - Node definitions with coordinates
- element_table.csv - Element definitions with connectivity
- construction_sequence.csv - Construction sequence by workfront
- workfront_step_table.csv - Step assignments per workfront
- validation_report.txt - Input data validation results
- stability_report.txt - Stability verification results
- metrics_summary.txt - Construction progress metrics
"""

import os
from datetime import datetime
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple, Union

from src.python.data_io import (
    save_element_table,
    save_node_table,
    save_step_table,
)
from src.python.sequence_table import save_sequence_table


# Type aliases
NodeTable = List[Tuple[int, float, float, float]]
ElementTable = List[Tuple[int, int, int, str]]
SequenceTable = List[Tuple[int, str]]  # (workfront_id, member_id)
StepTable = List[Tuple[int, int, str]]  # (workfront_id, step, member_id)


def ensure_output_folder(project_root: Union[str, Path]) -> Path:
    """Ensure output folder exists in project root.

    Args:
        project_root: Path to project root directory.

    Returns:
        Path to output folder.
    """
    output_path = Path(project_root) / "output"
    output_path.mkdir(parents=True, exist_ok=True)
    return output_path


def format_validation_report(
    nodes: NodeTable,
    elements: ElementTable,
    validation_errors: List[str],
) -> str:
    """Format validation results as human-readable text report.

    Args:
        nodes: List of (node_id, x, y, z) tuples.
        elements: List of (element_id, node_i_id, node_j_id, member_type) tuples.
        validation_errors: List of validation error messages (empty if all passed).

    Returns:
        Formatted validation report as string.
    """
    timestamp = datetime.now().strftime("%Y-%m-%d %H:%M:%S")

    # Count element types
    columns = [e for e in elements if e[3] == "Column"]
    girders = [e for e in elements if e[3] == "Girder"]

    # Get unique z-levels (floors)
    z_levels = sorted(set(z for _, _, _, z in nodes))

    lines = [
        "=" * 60,
        "입력 데이터 검증 결과 (Input Data Validation Report)",
        "=" * 60,
        f"생성 시각: {timestamp}",
        "",
        "-" * 60,
        "데이터 요약 (Data Summary)",
        "-" * 60,
        f"총 노드 수 (Total Nodes): {len(nodes)}",
        f"총 부재 수 (Total Elements): {len(elements)}",
        f"  - 기둥 (Columns): {len(columns)}",
        f"  - 거더 (Girders): {len(girders)}",
        f"층 수 (Number of Floors): {len(z_levels)}",
        f"층 레벨 (Floor Levels): {', '.join(f'{z:.1f}m' for z in z_levels)}",
        "",
        "-" * 60,
        "검증 결과 (Validation Results)",
        "-" * 60,
    ]

    if not validation_errors:
        lines.extend(
            [
                "상태: 통과 (PASSED)",
                "",
                "모든 검증 항목을 통과했습니다.",
                "All validation checks passed.",
                "",
                "검증 항목 (Validation Checks):",
                "  [✓] 중복 ID 검사 (Duplicate ID Check)",
                "  [✓] 영길이 부재 검사 (Zero-length Element Check)",
                "  [✓] 축 평행 검사 (Axis-parallel Check)",
                "  [✓] 대각선 부재 검사 (No Diagonal Check)",
                "  [✓] 고아 노드 검사 (Orphan Node Check)",
                "  [✓] 층 레벨 일관성 검사 (Floor Level Consistency)",
                "  [✓] 부재 중복 검사 (Overlapping Element Check)",
            ]
        )
    else:
        lines.extend(
            [
                "상태: 실패 (FAILED)",
                "",
                f"발견된 오류 수: {len(validation_errors)}",
                "",
                "오류 상세 (Error Details):",
            ]
        )
        for i, error in enumerate(validation_errors, 1):
            lines.append(f"  {i}. {error}")

    lines.extend(
        [
            "",
            "=" * 60,
        ]
    )

    return "\n".join(lines)


def save_validation_report(
    nodes: NodeTable,
    elements: ElementTable,
    validation_errors: List[str],
    output_folder: Union[str, Path],
) -> Path:
    """Save validation report to text file.

    Args:
        nodes: List of (node_id, x, y, z) tuples.
        elements: List of (element_id, node_i_id, node_j_id, member_type) tuples.
        validation_errors: List of validation error messages.
        output_folder: Path to output folder.

    Returns:
        Path to saved report file.
    """
    report = format_validation_report(nodes, elements, validation_errors)
    filepath = Path(output_folder) / "validation_report.txt"

    with open(filepath, "w", encoding="utf-8") as f:
        f.write(report)

    return filepath


def format_stability_report(
    verification_result: Dict[str, Any],
    nodes: NodeTable,
    elements: ElementTable,
) -> str:
    """Format stability verification results as human-readable text report.

    Args:
        verification_result: Result from verify_step_table_stability().
        nodes: List of (node_id, x, y, z) tuples.
        elements: List of (element_id, node_i_id, node_j_id, member_type) tuples.

    Returns:
        Formatted stability report as string.
    """
    timestamp = datetime.now().strftime("%Y-%m-%d %H:%M:%S")

    is_valid = verification_result.get("valid", False)
    step_results = verification_result.get("step_results", {})
    failed_steps = verification_result.get("failed_steps", [])
    floor_violations = verification_result.get("floor_violations", [])

    lines = [
        "=" * 60,
        "적합 및 안정 조건 검사 결과 (Stability Verification Report)",
        "=" * 60,
        f"생성 시각: {timestamp}",
        "",
        "-" * 60,
        "검사 결과 요약 (Summary)",
        "-" * 60,
        f"전체 결과: {'통과 (PASSED)' if is_valid else '실패 (FAILED)'}",
        f"검사한 Step 수: {len(step_results)}",
        f"실패한 Step 수: {len(failed_steps)}",
        f"층별 설치 제약 위반 수: {len(floor_violations)}",
        "",
    ]

    # Step-by-step results
    if step_results:
        lines.extend(
            [
                "-" * 60,
                "Step별 검사 결과 (Step-by-Step Results)",
                "-" * 60,
            ]
        )
        for step_num in sorted(step_results.keys()):
            result = step_results[step_num]
            status = "통과" if result.get("stable", False) else "실패"
            verified = result.get("elements_verified", 0)
            failed = result.get("failed_elements", [])
            lines.append(f"  Step {step_num}: {status} (검증된 부재: {verified}개)")
            if failed:
                lines.append(f"    - 실패 부재 ID: {', '.join(map(str, failed))}")
        lines.append("")

    # Floor violations
    if floor_violations:
        lines.extend(
            [
                "-" * 60,
                "층별 설치 제약 위반 (Floor Installation Violations)",
                "-" * 60,
            ]
        )
        for violation in floor_violations:
            lines.append(
                f"  Step {violation['step']}: 부재 {violation['element_id']} - "
                f"층 {violation['floor']} 설치 시도 "
                f"(하층 진행률: {violation['lower_floor_percentage']:.1f}%, "
                f"필요: {violation['required_threshold']:.1f}%)"
            )
        lines.append("")

    lines.extend(
        [
            "=" * 60,
        ]
    )

    return "\n".join(lines)


def save_stability_report(
    verification_result: Dict[str, Any],
    nodes: NodeTable,
    elements: ElementTable,
    output_folder: Union[str, Path],
) -> Path:
    """Save stability verification report to text file.

    Args:
        verification_result: Result from verify_step_table_stability().
        nodes: List of (node_id, x, y, z) tuples.
        elements: List of (element_id, node_i_id, node_j_id, member_type) tuples.
        output_folder: Path to output folder.

    Returns:
        Path to saved report file.
    """
    report = format_stability_report(verification_result, nodes, elements)
    filepath = Path(output_folder) / "stability_report.txt"

    with open(filepath, "w", encoding="utf-8") as f:
        f.write(report)

    return filepath


def format_metrics_report(
    metrics: Dict[str, Any],
) -> str:
    """Format metrics summary as human-readable text report.

    Args:
        metrics: Result from get_metrics_summary().

    Returns:
        Formatted metrics report as string.
    """
    timestamp = datetime.now().strftime("%Y-%m-%d %H:%M:%S")

    installed = metrics.get("installed", {})
    total = metrics.get("total", {})
    progress = metrics.get("progress", {})
    floor_pct = metrics.get("floor_percentages", {})

    lines = [
        "=" * 60,
        "공사 진행 현황 (Construction Progress Metrics)",
        "=" * 60,
        f"생성 시각: {timestamp}",
        "",
        "-" * 60,
        "부재 설치 현황 (Installation Status)",
        "-" * 60,
        f"기둥 (Columns): {installed.get('columns', 0)} / {total.get('columns', 0)} ({progress.get('columns_pct', 0):.1f}%)",
        f"거더 (Girders): {installed.get('girders', 0)} / {total.get('girders', 0)} ({progress.get('girders_pct', 0):.1f}%)",
        f"전체 (Total): {installed.get('total', 0)} / {total.get('total', 0)} ({progress.get('total_pct', 0):.1f}%)",
        "",
    ]

    if floor_pct:
        lines.extend(
            [
                "-" * 60,
                "층별 기둥 설치율 (Floor-level Column Installation)",
                "-" * 60,
            ]
        )
        for floor in sorted(floor_pct.keys()):
            pct = floor_pct[floor]
            bar_length = int(pct / 5)  # 20 chars = 100%
            bar = "█" * bar_length + "░" * (20 - bar_length)
            lines.append(f"  층 {floor}: [{bar}] {pct:.1f}%")
        lines.append("")

    lines.extend(
        [
            "=" * 60,
        ]
    )

    return "\n".join(lines)


def format_step_statistics_report(
    step_stats: Dict[str, Any],
) -> str:
    """Format step-by-step construction statistics as human-readable text report.

    Args:
        step_stats: Result from calculate_step_statistics().

    Returns:
        Formatted step statistics report as string.
    """
    timestamp = datetime.now().strftime("%Y-%m-%d %H:%M:%S")

    steps = step_stats.get("steps", {})
    total_elements = step_stats.get("total_elements", 0)
    total_columns = step_stats.get("total_columns", 0)
    total_girders = step_stats.get("total_girders", 0)

    lines = [
        "=" * 70,
        "시공 단계별 통계 (Step-by-Step Construction Statistics)",
        "=" * 70,
        f"생성 시각: {timestamp}",
        "",
        "-" * 70,
        "전체 모델 요약 (Model Summary)",
        "-" * 70,
        f"총 부재 수 (Total Elements): {total_elements}",
        f"  - 기둥 (Columns): {total_columns}",
        f"  - 거더 (Girders): {total_girders}",
        "",
    ]

    if steps:
        lines.extend(
            [
                "-" * 70,
                "단계별 설치 현황 (Step-by-Step Installation)",
                "-" * 70,
                "",
            ]
        )

        for step_num in sorted(steps.keys()):
            step_data = steps[step_num]
            step_cols = step_data.get("step_columns", 0)
            step_gird = step_data.get("step_girders", 0)
            step_total = step_data.get("step_total", 0)
            cum_cols = step_data.get("cumulative_columns", 0)
            cum_gird = step_data.get("cumulative_girders", 0)
            cum_total = step_data.get("cumulative_total", 0)
            floor_pct = step_data.get("floor_percentages", {})
            step_floor_cols = step_data.get("step_floor_columns", {})

            # Progress bar for cumulative
            if total_elements > 0:
                progress_pct = (cum_total / total_elements) * 100
                bar_length = int(progress_pct / 5)  # 20 chars = 100%
                bar = "█" * bar_length + "░" * (20 - bar_length)
            else:
                progress_pct = 0.0
                bar = "░" * 20

            lines.append(f"┌{'─' * 68}┐")
            lines.append(
                f"│ Step {step_num:3d}                                                          │"
            )
            lines.append(f"├{'─' * 68}┤")

            # This step's installation
            lines.append(
                f"│ 이번 단계 설치 (This Step):                                         │"
            )
            lines.append(
                f"│   기둥: {step_cols:4d}개, 거더: {step_gird:4d}개, 합계: {step_total:4d}개                  │"
            )

            # Floor-level columns installed in this step
            if step_floor_cols:
                floor_detail = ", ".join(
                    [f"층{f}: {c}개" for f, c in sorted(step_floor_cols.items())]
                )
                # Truncate if too long
                if len(floor_detail) > 50:
                    floor_detail = floor_detail[:47] + "..."
                lines.append(f"│   층별 기둥: {floor_detail:<54} │")

            lines.append(
                f"│                                                                      │"
            )

            # Cumulative totals
            lines.append(
                f"│ 누적 합계 (Cumulative Total):                                        │"
            )
            lines.append(
                f"│   기둥: {cum_cols:4d}/{total_columns:<4d}, 거더: {cum_gird:4d}/{total_girders:<4d}, 합계: {cum_total:4d}/{total_elements:<4d}       │"
            )
            lines.append(
                f"│   진행률: [{bar}] {progress_pct:5.1f}%                    │"
            )

            lines.append(
                f"│                                                                      │"
            )

            # Floor percentages after this step
            lines.append(
                f"│ 층별 기둥 설치율 (Floor Column Installation %):                     │"
            )
            if floor_pct:
                for floor in sorted(floor_pct.keys()):
                    pct = floor_pct[floor]
                    mini_bar_len = int(pct / 10)  # 10 chars = 100%
                    mini_bar = "█" * mini_bar_len + "░" * (10 - mini_bar_len)
                    lines.append(
                        f"│   층 {floor:2d}: [{mini_bar}] {pct:5.1f}%                                       │"
                    )
            else:
                lines.append(
                    f"│   (데이터 없음)                                                      │"
                )

            lines.append(f"└{'─' * 68}┘")
            lines.append("")

    lines.extend(
        [
            "=" * 70,
        ]
    )

    return "\n".join(lines)


def save_step_statistics_report(
    step_stats: Dict[str, Any],
    output_folder: Union[str, Path],
) -> Path:
    """Save step statistics report to text file.

    Args:
        step_stats: Result from calculate_step_statistics().
        output_folder: Path to output folder.

    Returns:
        Path to saved report file.
    """
    report = format_step_statistics_report(step_stats)
    filepath = Path(output_folder) / "step_statistics.txt"

    with open(filepath, "w", encoding="utf-8") as f:
        f.write(report)

    return filepath


def save_metrics_report(
    metrics: Dict[str, Any],
    output_folder: Union[str, Path],
) -> Path:
    """Save metrics summary report to text file.

    Args:
        metrics: Result from get_metrics_summary().
        output_folder: Path to output folder.

    Returns:
        Path to saved report file.
    """
    report = format_metrics_report(metrics)
    filepath = Path(output_folder) / "metrics_summary.txt"

    with open(filepath, "w", encoding="utf-8") as f:
        f.write(report)

    return filepath


def export_all_outputs(
    project_root: Union[str, Path],
    nodes: NodeTable,
    elements: ElementTable,
    sequence: Optional[SequenceTable] = None,
    step_table: Optional[StepTable] = None,
    validation_errors: Optional[List[str]] = None,
    stability_result: Optional[Dict[str, Any]] = None,
    metrics: Optional[Dict[str, Any]] = None,
    step_statistics: Optional[Dict[str, Any]] = None,
) -> Dict[str, Path]:
    """Export all analysis outputs to the output folder.

    This is the main entry point for saving all results to files
    for user review. Creates output/ folder if it doesn't exist.

    Args:
        project_root: Path to project root directory.
        nodes: List of (node_id, x, y, z) tuples.
        elements: List of (element_id, node_i_id, node_j_id, member_type) tuples.
        sequence: Optional list of (workfront_id, member_id) tuples.
        step_table: Optional list of (workfront_id, step, member_id) tuples.
        validation_errors: Optional list of validation error messages.
        stability_result: Optional result from verify_step_table_stability().
        metrics: Optional result from get_metrics_summary().
        step_statistics: Optional result from calculate_step_statistics().

    Returns:
        Dictionary mapping output type to saved file path.

    Example:
        >>> saved = export_all_outputs(
        ...     project_root=".",
        ...     nodes=nodes,
        ...     elements=elements,
        ...     sequence=sequence,
        ...     step_table=step_table,
        ...     validation_errors=[],
        ... )
        >>> print(saved["node_table"])  # Path to saved node table
    """
    output_folder = ensure_output_folder(project_root)
    saved_files: Dict[str, Path] = {}

    # Save node table
    node_path = output_folder / "node_table.csv"
    save_node_table(nodes, str(node_path), format="csv")
    saved_files["node_table"] = node_path

    # Save element table
    element_path = output_folder / "element_table.csv"
    save_element_table(elements, str(element_path), format="csv")
    saved_files["element_table"] = element_path

    # Save construction sequence (if provided)
    if sequence:
        sequence_path = output_folder / "construction_sequence.csv"
        save_sequence_table(sequence, str(sequence_path), format="csv")
        saved_files["construction_sequence"] = sequence_path

    # Save workfront step table (if provided)
    if step_table:
        step_path = output_folder / "workfront_step_table.csv"
        save_step_table(step_table, str(step_path), format="csv")
        saved_files["workfront_step_table"] = step_path

    # Save validation report (if validation was performed)
    if validation_errors is not None:
        report_path = save_validation_report(
            nodes, elements, validation_errors, output_folder
        )
        saved_files["validation_report"] = report_path

    # Save stability report (if stability check was performed)
    if stability_result is not None:
        stability_path = save_stability_report(
            stability_result, nodes, elements, output_folder
        )
        saved_files["stability_report"] = stability_path

    # Save metrics report (if metrics were calculated)
    if metrics is not None:
        metrics_path = save_metrics_report(metrics, output_folder)
        saved_files["metrics_report"] = metrics_path

    # Save step statistics report (if step statistics were calculated)
    if step_statistics is not None:
        step_stats_path = save_step_statistics_report(step_statistics, output_folder)
        saved_files["step_statistics"] = step_stats_path

    return saved_files


def get_output_folder_path(project_root: Union[str, Path]) -> Path:
    """Get the path to the output folder.

    Args:
        project_root: Path to project root directory.

    Returns:
        Path to output folder (may not exist yet).
    """
    return Path(project_root) / "output"
