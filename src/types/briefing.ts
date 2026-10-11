export interface Briefing {
  enabled: boolean;
  /** "HH:MM", local. */
  time: string;
  weekdaysOnly: boolean;
  weather: boolean;
  calendar: boolean;
  email: boolean;
  tasks: boolean;
  news: string[];
  routineId: number | null;
}

export interface Place {
  name: string;
  latitude: number;
  longitude: number;
}
