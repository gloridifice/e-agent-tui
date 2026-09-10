## MODIFIED Requirements

### Requirement: Resume title and modification date rows
Resume rows SHALL omit session paths and display a left-aligned title in the theme's Mist-equivalent tone and an available last-modified age right-aligned in its Bark-equivalent tone. The age SHALL express elapsed time from modification to now using integer `d`, `h`, `m`, and `s` units with no spaces or decimal points. It SHALL retain at most the largest unit and its adjacent smaller unit, omit a zero remainder, and discard finer precision. Seconds SHALL be the smallest unit; subsecond and future modification times SHALL display `0s`. Visible ages SHALL refresh as their displayed value changes without rescanning files. A focus marker SHALL identify the selected row without overriding either text tone. Title truncation SHALL reserve room for the age and separate the columns when space permits. A backend that does not provide last-modified metadata SHALL leave the age absent rather than mislabel creation time as modification time. Paths SHALL remain usable for identity filtering and attachment despite being omitted from row presentation.

#### Scenario: Focused dated row
- **WHEN** a Resume row with modification metadata is focused
- **THEN** its title remains Mist-equivalent, its relative age remains Bark-equivalent at the right edge, and a separate marker identifies focus without showing the session path

#### Scenario: Compact relative ages
- **WHEN** sessions were modified five seconds, ten minutes, thirteen hours, or three days and two hours ago
- **THEN** their labels are respectively `5s`, `10m`, `13h`, and `3d2h`, without calendar timestamps or decimal points

#### Scenario: Visible age advances
- **WHEN** an open Resume row's modification age crosses its displayed precision boundary
- **THEN** its relative label refreshes without reloading session metadata or moving selection

#### Scenario: Clock skew or subsecond age
- **WHEN** a session's modification timestamp is in the future or less than one second old
- **THEN** its label is `0s` rather than a negative or fractional duration

#### Scenario: Narrow row
- **WHEN** a title and age cannot fit in the available row width
- **THEN** the title is truncated before the age and neither column wraps or overlaps neighboring rows

#### Scenario: Missing modification date
- **WHEN** a backend provides a session without last-modified metadata
- **THEN** the title remains visible with no fabricated modification age
